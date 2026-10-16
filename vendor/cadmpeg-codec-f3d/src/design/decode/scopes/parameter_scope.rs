// SPDX-License-Identifier: Apache-2.0
//! Decode parameter scopes and parse one scope payload.

use crate::bytes::{lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
use cadmpeg_core::decode::u64_from_index;

use super::assembly_alignment::exact_assembly_alignment;
use super::axial_assembly::bind_axial_assembly_operand_targets;
use super::axial_assembly::bind_joint_origin_frames_from_assemblies;
use super::base_feature::exact_base_feature_construction;
use super::coil::bind_coil_extent_from_parameters;
use super::coil::exact_coil_discriminators;
use super::coil::exact_coil_placement;
use super::coil::exact_long_coil_transform;
use super::combine::exact_combine_operation;
use super::component_constructions::bind_component_pattern_occurrences;
use super::component_constructions::exact_component_insert_construction;
use super::component_constructions::exact_copy_paste_component_operation;
use super::component_constructions::exact_derived_instance_construction;
use super::copy_paste_bodies::exact_copy_paste_bodies_operation;
use super::direct_face::exact_direct_face_operation;
use super::direct_face::exact_move_operation;
use super::direct_face::exact_scale_operation;
use super::draft::exact_draft_operation_with_owners;
use super::extrude::exact_extrude_prologue;
use super::fixed_parameters::exact_fixed_chamfer_parameters;
use super::fixed_parameters::exact_fixed_extrude_parameters;
use super::fixed_parameters::exact_fixed_fillet_parameters;
use super::hole::exact_hole_construction;
use super::path_feature::exact_path_feature_construction;
use super::pattern::exact_circular_pattern_construction_with_owners;
use super::pattern::exact_rectangular_pattern_construction;
use super::point_data::exact_work_point_construction;
use super::sheet_metal::bind_hem_operation_from_parameters;
use super::sheet_metal::exact_base_flange_operation;
use super::sheet_metal::exact_edge_flange_operation;
use super::solid_primitive::exact_solid_primitive;
use super::surfaces::exact_ruled_surface_operation;
use super::surfaces::exact_surface_extend_operation;
use super::surfaces::exact_surface_offset_operation;
use super::surfaces::exact_surface_stitch_operation;
use super::thread::exact_thread_construction;
use super::work_geometry::exact_joint_origin_frame;
use super::work_geometry::exact_work_axis_construction;
use super::work_geometry::exact_work_plane_frame;
use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::assembly::exact_legacy_as_built_421_operands;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::sketch::{native_scope_charged, IndexedRecordOffsets};
use crate::design::decode::text::design_record_id_charged;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids;
use crate::ids::native_stream;
use crate::records::feature::assembly;
use crate::records::feature::coil;
use crate::records::feature::direct_face;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub(crate) fn decode_parameter_scopes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    native: &crate::native::F3dNative,
) -> Result<Vec<DesignParameterScope>, CodecError> {
    let entities = &native.design_entity_headers;
    let types = &native.design_types;
    let parameters = &native.design_parameters;
    let parameter_owners = &native.design_parameter_owners;
    let component_occurrences = &native.design_component_occurrences;
    let recipes = &native.construction_recipes;
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = native_scope_charged(ctx, &entry.name)?;
        let records = IndexedRecordOffsets::build(ctx, bytes)?;
        let stream_types =
            crate::design::decode::meta::stream_types_by_entity(ctx, types, &entry.name)?;
        let stream_scope_start = out.len();
        for header in parameter_scope_candidate_headers(ctx, bytes, &records)? {
            let Some(mut scope) = parse_parameter_scope(
                ctx,
                bytes,
                &records,
                header.record_index,
                &header.class_tag,
                header.byte_offset,
            )?
            else {
                continue;
            };
            scope.id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-parameter-scope#",
                scope.byte_offset(),
                "f3d Design parameter scope ID",
            )?;
            bind_coil_extent_from_parameters(ctx, &mut scope, parameters, parameter_owners)?;
            bind_hem_operation_from_parameters(bytes, &mut scope, parameters, parameter_owners);
            if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch) {
                let start = usize::try_from(scope.byte_offset()).ok();
                let end = usize::try_from(scope.paired_byte_offset()).ok();
                let frame = start
                    .zip(end)
                    .and_then(|(start, end)| bytes.get(start..end));
                let mut unique_match = None;
                let mut multiple_matches = false;
                if let Some(frame) = frame {
                    // One pass over the frame: the first offset of every
                    // marked reference (a one byte, a u32 suffix, six zero
                    // bytes), then each eligible entity looks its suffix up.
                    let first_at = first_marked_reference_offsets(ctx, frame)?;
                    for entity in entities {
                        if native_stream(&entity.id) != Some(stream.as_str())
                            || !entity.in_sketch_module()
                            || entity.entity_id.suffix() > u64::from(u32::MAX)
                        {
                            continue;
                        }
                        let Ok(suffix) = u32::try_from(entity.entity_id.suffix()) else {
                            continue;
                        };
                        if let Some(at) = first_at.get(&suffix) {
                            if unique_match.replace((entity, at + 1)).is_some() {
                                multiple_matches = true;
                            }
                        }
                    }
                }
                if let Some((entity, relative_offset)) = unique_match.filter(|_| !multiple_matches)
                {
                    let Some(entity_reference_offset) = scope
                        .byte_offset()
                        .checked_add(u64_from_index(relative_offset))
                    else {
                        continue;
                    };
                    if let scope::DesignScopePayloadMut::Sketch(slot)
                    | scope::DesignScopePayloadMut::Esquisse(slot)
                    | scope::DesignScopePayloadMut::Skizze(slot)
                    | scope::DesignScopePayloadMut::Esboco(slot) = scope.payload_mut()
                    {
                        let entity_id = copy_sketch_entity_id(ctx, &entity.entity_id)?;
                        *slot = Some(scope::DesignSketchEntityBinding {
                            entity_id,
                            entity_reference_offset,
                        });
                    }
                }
            }
            if scope.kind() == scope::DesignFeatureKind::WorkPlane {
                if let Some(frame) = exact_work_plane_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
                        *slot = Some(scope::DesignWorkPlaneTransform {
                            work_plane_transform: frame.transform,
                            work_plane_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignWorkPlaneReference {
                                    work_plane_reference: record_index,
                                    work_plane_reference_offset: offset,
                                }
                            }),
                            work_plane_construction: None,
                        });
                    }
                }
            }
            if let Some(construction) = exact_work_axis_construction(bytes, &records, &scope) {
                if let scope::DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() {
                    *slot = Some(construction);
                }
            }
            if scope.kind() == scope::DesignFeatureKind::JointOrigin {
                if let Some(frame) = exact_joint_origin_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
                        *slot = Some(scope::DesignJointOriginTransform {
                            joint_origin_transform: frame.transform,
                            joint_origin_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignJointOriginReference {
                                    joint_origin_reference: record_index,
                                    joint_origin_reference_offset: offset,
                                }
                            }),
                        });
                    }
                }
            }
            {
                let construction =
                    exact_work_point_construction(ctx, bytes, &records, &scope, &stream_types)?;
                if let scope::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_hole_construction(
                    ctx,
                    bytes,
                    &records,
                    &scope,
                    &stream_types,
                    &scope::DesignFeatureKind::Hole,
                )?;
                if let scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            if let Some(placement) = exact_coil_placement(ctx, bytes, &records, &scope, recipes)? {
                if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
                | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
                {
                    slot.get_or_insert_with(Default::default).placement = Some(placement);
                }
            }
            if let Some(construction) =
                exact_solid_primitive(bytes, &records, &scope, parameter_owners)
            {
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::SpherePrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Sphere(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::TorusPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Torus(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::BoxPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Box(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::CylinderPrimitive(slot),
                        crate::records::feature::primitives::DesignSolidPrimitive::Cylinder(value),
                    ) => *slot = Some(value),
                    _ => {
                        return Err(CodecError::NotImplemented(
                            "F3D solid primitive payload kind mismatch".into(),
                        ))
                    }
                }
            }
            {
                let construction = exact_direct_face_operation(bytes, &records, &scope);
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::OffsetFaces(slot)
                        | scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                        Some(direct_face::DesignDirectFaceOperation::OffsetFaces(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Shell(slot)
                        | scope::DesignScopePayloadMut::Schale(slot),
                        Some(direct_face::DesignDirectFaceOperation::Shell(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Thicken(slot),
                        Some(direct_face::DesignDirectFaceOperation::Thicken(value)),
                    ) => *slot = Some(value),
                    _ => {}
                }
            }
            {
                let construction = exact_move_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::Move(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_scale_operation(ctx, bytes, &records, &scope, &stream_types)?;
                if let scope::DesignScopePayloadMut::Scale(slot)
                | scope::DesignScopePayloadMut::Massstab(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_extend_operation(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::SurfaceExtend(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_offset_operation(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::SurfaceOffset(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            if let Some(parameters) = exact_fixed_extrude_parameters(
                bytes,
                &records,
                &scope,
                parameters,
                parameter_owners,
            ) {
                {
                    let value = Some(parameters);
                    if let scope::DesignScopePayloadMut::Extrude(slot)
                    | scope::DesignScopePayloadMut::Extrusion(slot)
                    | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
                    {
                        slot.get_or_insert_with(Default::default)
                            .fixed_extrude_parameters = value;
                    }
                }
            }
            {
                let construction = exact_fixed_fillet_parameters(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::Fillet(slot)
                | scope::DesignScopePayloadMut::Conge(slot)
                | scope::DesignScopePayloadMut::Abrundung(slot)
                | scope::DesignScopePayloadMut::Arredondamento(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_fixed_chamfer_parameters(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Chamfer(slot)
                | scope::DesignScopePayloadMut::Chanfrein(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            if let Some(construction) =
                exact_path_feature_construction(bytes, &records, &scope, parameter_owners)
            {
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::Revolve(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Revolve(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Loft(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Loft(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Pipe(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Pipe(value),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Sweep(slot),
                        crate::records::feature::path_features::DesignPathFeatureConstruction::Sweep(value),
                    ) => *slot = Some(scope::DesignSweepScope {
                        construction: Some(value),
                        sweep_profile: None,
                    }),
                    _ => return Err(CodecError::NotImplemented("F3D path feature payload kind mismatch".into())),
                }
            }
            {
                let construction = exact_combine_operation(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_thread_construction(ctx, bytes, &scope)?;
                if let scope::DesignScopePayloadMut::Thread(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_draft_operation_with_owners(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_circular_pattern_construction_with_owners(
                    ctx,
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                )?;
                if let scope::DesignScopePayloadMut::CPattern(slot)
                | scope::DesignScopePayloadMut::CircularPattern(slot)
                | scope::DesignScopePayloadMut::ReseauC(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_rectangular_pattern_construction(
                    ctx,
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                )?;
                if let scope::DesignScopePayloadMut::RPattern(slot)
                | scope::DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_assembly_alignment(ctx, bytes, &records, &scope, parameter_owners)?;
                if let scope::DesignScopePayloadMut::Assemble(slot)
                | scope::DesignScopePayloadMut::AsBuilt(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            let legacy_form = scope
                .assembly_alignment()
                .and_then(|alignment| {
                    let assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                        solved_frame,
                        limits,
                    } = alignment.form.as_ref()?
                    else {
                        return None;
                    };
                    let carriers = match exact_legacy_as_built_421_operands(
                        ctx,
                        bytes,
                        &records,
                        &scope,
                        &stream_types,
                        recipes,
                        solved_frame,
                    ) {
                        Ok(Some(carriers)) => carriers,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    Some(Ok(
                        assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            carriers,
                            solved_frame: solved_frame.clone(),
                            limits: limits.clone(),
                            frames_field_present: true,
                        },
                    ))
                })
                .transpose()?;
            if let (Some(alignment), Some(form)) = (scope.assembly_alignment_mut(), legacy_form) {
                alignment.form = Some(form);
            }
            {
                let construction =
                    exact_component_insert_construction(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_derived_instance_construction(
                    ctx,
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                )?;
                if let scope::DesignScopePayloadMut::DerivedInstance(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_copy_paste_component_operation(
                    ctx,
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                )?;
                if let scope::DesignScopePayloadMut::CopyPaste(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            bind_component_pattern_occurrences(ctx, &mut scope, component_occurrences)?;
            {
                let construction = exact_copy_paste_bodies_operation(ctx, bytes, &records, &scope)?;
                if let scope::DesignScopePayloadMut::CopyPasteBodies(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_base_feature_construction(ctx, bytes, &scope)?;
                if let scope::DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }

            ctx.reserve_vec(&mut out, 1, "f3d Design parameter scopes")?;
            out.push(scope);
        }
        bind_joint_origin_frames_from_assemblies(ctx, bytes, &mut out[stream_scope_start..])?;
        bind_axial_assembly_operand_targets(ctx, bytes, &records, &mut out[stream_scope_start..])?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design parameter_scope 1",
    )?;
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

fn first_marked_reference_offsets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &[u8],
) -> Result<HashMap<u32, usize>, CodecError> {
    let mut first_at = HashMap::new();
    for at in memchr::memchr_iter(1, frame) {
        if at + 11 <= frame.len() && frame[at + 5..at + 11] == [0; 6] {
            if let Some(suffix) = View::u32_le_at(frame, at + 1) {
                if !first_at.contains_key(&suffix) {
                    ctx.reserve_map(&mut first_at, 1, "f3d Sketch scope reference offsets")?;
                    first_at.insert(suffix, at);
                }
            }
        }
    }
    Ok(first_at)
}

fn copy_sketch_entity_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &crate::records::identity::DesignEntityId,
) -> Result<crate::records::identity::DesignEntityId, CodecError> {
    let text = String::from_utf8(
        ctx.copy_retained(source.as_str().as_bytes(), "f3d Sketch scope entity ID")?,
    )
    .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
    crate::records::identity::DesignEntityId::try_from(text).map_err(CodecError::NotImplemented)
}

/// Admit one envelope for every logical scope identity.
///
/// Some Design streams retain more than one complete envelope for one record
/// index. A history-bound envelope is authoritative when exactly one
/// candidate resolves to a unique ASM state transition; an unresolved group
/// remains an error so a duplicate cannot be selected by byte order.
pub(crate) fn admit_history_bound_scope_variants(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut Vec<DesignParameterScope>,
    histories: &[crate::history_records::AsmHistory],
) -> Result<(), CodecError> {
    let (mut admitted, _admitted_storage) =
        ctx.temporary_vec(scopes.len(), "f3d scope admission")?;
    admitted.extend(std::iter::repeat_n(true, scopes.len()));
    let mut group_storage = ctx.reserve_scoped(0, "f3d scope admission groups")?;
    let mut groups = HashMap::<(&str, u32), Vec<usize>>::new();
    for (index, scope) in scopes.iter().enumerate() {
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let key = (stream, scope.record_index);
        group_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut groups,
                key,
                index,
                "f3d scope admission groups",
                "f3d scope admission group indices",
            )
        })?;
    }

    for indices in groups.values() {
        let [first, following @ ..] = indices.as_slice() else {
            continue;
        };
        if following.is_empty() {
            continue;
        }
        let mut history_bound = None;
        let mut multiple_history_bounds = false;
        for index in indices {
            let scope = &scopes[*index];
            let Some(state_id) = scope.history_state_id() else {
                continue;
            };
            let Some(previous_state_id) =
                crate::history::effective_scope_previous_history_state_id(scope, histories)
            else {
                continue;
            };
            if crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
                .is_some()
                && history_bound.replace(*index).is_some()
            {
                multiple_history_bounds = true;
                break;
            }
        }
        let mut equivalent_payload = history_bound.is_none() && !multiple_history_bounds;
        if equivalent_payload {
            for index in following {
                if !equivalent_scope_variant_payload(ctx, &scopes[*first], &scopes[*index])? {
                    equivalent_payload = false;
                    break;
                }
            }
        }
        let keep = match (history_bound, multiple_history_bounds) {
            (Some(keep), false) => keep,
            (None, false) if equivalent_payload => {
                following.iter().copied().fold(*first, |keep, index| {
                    if scopes[index].byte_offset() >= scopes[keep].byte_offset() {
                        index
                    } else {
                        keep
                    }
                })
            }
            _ => {
                return Err(CodecError::Malformed(
                    "Design scope record identity has unresolved duplicate envelopes".into(),
                ));
            }
        };
        for index in indices {
            admitted[*index] = *index == keep;
        }
    }

    drop(groups);
    drop(group_storage);
    let retained_count = admitted.iter().filter(|selected| **selected).count();

    let mut retained = Vec::new();
    ctx.reserve_vec(
        &mut retained,
        retained_count,
        "f3d scope admission retained output",
    )?;
    for (index, scope) in std::mem::take(scopes).into_iter().enumerate() {
        if admitted[index] {
            retained.push(scope);
        }
    }
    *scopes = retained;
    Ok(())
}

/// Count serialized JSON bytes without retaining the serialized document.
#[derive(Default)]
struct JsonByteCounter {
    bytes: usize,
}

impl std::io::Write for JsonByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("scope JSON length overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Compare two same-index scope envelopes after removing source-location and
/// dynamic-class fields. An equivalent envelope is one serialization of the
/// same logical scope; the later envelope supersedes the earlier one when no
/// decoded ASM state pair can select a revision.
fn equivalent_scope_variant_payload(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: &DesignParameterScope,
    right: &DesignParameterScope,
) -> Result<bool, CodecError> {
    let mut left_count = JsonByteCounter::default();
    let mut right_count = JsonByteCounter::default();
    if serde_json::to_writer(&mut left_count, left).is_err()
        || serde_json::to_writer(&mut right_count, right).is_err()
    {
        return Ok(false);
    }
    let serialized = left_count
        .bytes
        .checked_add(right_count.bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("f3d scope variant JSON size", 0, 1))?;
    let work = u64_from_index(serialized)
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("f3d scope variant comparison work", 0, 1))?;
    ctx.charge_work(work, "f3d scope variant comparison")?;
    let scratch_bytes = serialized
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(16))
        .ok_or_else(|| ctx.refuse_codec_limit("f3d scope variant JSON size", 0, 1))?;
    let _scratch = ctx.reserve_scoped(u64_from_index(scratch_bytes), "f3d scope variant JSON")?;
    let (equivalent, _storage) = ctx.with_scoped_storage("f3d scope variant JSON", || {
        let Some(mut left) = scope_variant_json(ctx, left, left_count.bytes)? else {
            return Ok(false);
        };
        let Some(mut right) = scope_variant_json(ctx, right, right_count.bytes)? else {
            return Ok(false);
        };
        strip_scope_variant_provenance(ctx, &mut left, true)?;
        strip_scope_variant_provenance(ctx, &mut right, true)?;
        Ok::<_, CodecError>(left == right)
    })?;
    Ok(equivalent)
}

struct ScopeJsonWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for ScopeJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|length| *length <= self.limit)
            .ok_or_else(|| std::io::Error::other("scope JSON changed during serialization"))?;
        if length > self.bytes.capacity() {
            return Err(std::io::Error::other("scope JSON exceeds admitted storage"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn scope_variant_json(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &DesignParameterScope,
    serialized_length: usize,
) -> Result<Option<serde_json::Value>, CodecError> {
    use serde::de::DeserializeSeed;

    let bytes = ctx.collection_vec(serialized_length, "f3d scope variant JSON")?;
    let mut writer = ScopeJsonWriter {
        bytes,
        limit: serialized_length,
    };
    if serde_json::to_writer(&mut writer, scope).is_err() {
        return Ok(None);
    }
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(&writer.bytes);
    let parsed = crate::design::json_value::ValueSeed {
        ctx,
        refusal: &mut refusal,
        member: false,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(Some(value)),
        Err(_) => match refusal {
            Some(error) => Err(error),
            None => Ok(None),
        },
    }
}

fn strip_scope_variant_provenance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    top_level: bool,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("f3d scope variant provenance depth")?;
    ctx.charge_work(1, "f3d scope variant provenance work")?;
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                strip_scope_variant_provenance(ctx, item, false)?;
            }
        }
        serde_json::Value::Object(fields) => {
            fields.retain(|key, _| {
                if key.ends_with("_offset") || key.ends_with("_offsets") {
                    return false;
                }
                if top_level
                    && matches!(
                        key.as_str(),
                        "id" | "class_tag"
                            | "frame_length"
                            | "history_state_id"
                            | "previous_history_state_id"
                            | "paired_class_tag"
                    )
                {
                    return false;
                }
                true
            });
            for field in fields.values_mut() {
                strip_scope_variant_provenance(ctx, field, false)?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}

/// Skip the payload prologue at `at`: a leading-block presence byte, a property
/// presence byte, and the property block that byte gates. The leading-block
/// byte belongs to classes that write one, so this reader only steps over it.
pub(in crate::design::decode) fn payload_prologue(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<usize> {
    fn graphic_ascii_at(bytes: &[u8], at: usize) -> Option<(&[u8], usize)> {
        let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if !(1..=64).contains(&length) {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(length)?;
        let raw = bytes.get(start..end)?;
        raw.iter().all(u8::is_ascii_graphic).then_some((raw, end))
    }

    let mut cursor = at.checked_add(1)?;
    let present = *bytes.get(cursor)?;
    cursor += 1;
    match present {
        0 => Some(cursor),
        1 => {
            let count = View::u32_le_at(bytes, cursor)?;
            if count > 16 {
                return None;
            }
            cursor += 4;
            for _ in 0..count {
                let (_key, after_key) = graphic_ascii_at(bytes, cursor)?;
                let (type_name, after_type) = graphic_ascii_at(bytes, after_key)?;
                if type_name != b"IntrinsicMetaTypeuint64" {
                    return None;
                }
                cursor = after_type.checked_add(8)?;
            }
            (cursor <= end).then_some(cursor)
        }
        _ => None,
    }
}

/// Every indexed-record header that can open a parameter scope: a scope is
/// delimited by two headers carrying its record index, so the last header of an
/// index opens nothing.
pub(super) fn parameter_scope_candidate_headers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Result<Vec<RecordFrame>, CodecError> {
    let mut headers = Vec::new();
    for (record_index, offsets) in records.records() {
        for at in offsets.windows(2).map(|pair| &pair[0]) {
            let Some((class_tag, _)) =
                lp_ascii_filtered_view(bytes, *at, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag) else {
                continue;
            };
            let byte_offset = u64::try_from(*at)
                .map_err(|_| ctx.refuse_codec_limit("f3d Design scope header offset", 0, 1))?;

            ctx.reserve_vec(&mut headers, 1, "f3d Design scope candidate headers")?;
            headers.push(RecordFrame {
                record_index,
                class_tag,
                byte_offset,
            });
        }
    }
    Ok(headers)
}

pub(crate) fn parameter_scope_tail_length_is_valid(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> bool {
    let kind = kind.as_ref();
    if (80..=590).contains(&tail_length) && tail_length.is_multiple_of(2) {
        return true;
    }
    match kind {
        "CopyPasteBodies" => tail_length == 110,
        "CoilPrimitive" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 88),
        _ => matches!(tail_length, 72 | 76 | 77 | 78 | 87),
    }
}

pub(crate) fn parameter_scope_previous_history_offset(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(
        kind.as_ref(),
        tail_length,
        ScopeTailForm::Fixed,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeTailForm {
    Fixed,
    Named,
}

fn parameter_scope_previous_history_offset_for_form(
    kind: &str,
    tail_length: usize,
    tail_form: ScopeTailForm,
) -> Option<usize> {
    if tail_form == ScopeTailForm::Named {
        return None;
    }
    match (kind, tail_length) {
        ("CopyPasteBodies", 110) => Some(53),
        ("CoilPrimitive", 88) => None,
        (_, 72 | 76) => Some(30),
        (_, 77 | 78) => Some(31),
        (_, 87) => Some(41),
        _ => None,
    }
}

pub(in crate::design::decode) fn parse_parameter_scope(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &crate::records::references::DesignClassTag,
    byte_offset: u64,
) -> Result<Option<DesignParameterScope>, CodecError> {
    (|| {
        let start = usize::try_from(byte_offset).ok()?;
        let paired_at = records.first_at_or_after(start.checked_add(11)?, record_index)?;
        let (paired_class_tag, _) =
            lp_ascii_filtered_view(bytes, paired_at, 3..=3, u8::is_ascii_digit)?;
        let mut fixed_candidate = None;
        let mut fixed_ambiguous = false;
        let mut named_candidate = None;
        let mut named_ambiguous = false;
        let kind_scan_end = paired_at.checked_sub(72)?;
        for at in (start.checked_add(11)?..paired_at)
            .rev()
            .take(590 + 4 + 2 * 256)
            .rev()
            .take_while(|at| *at < kind_scan_end)
        {
            let (kind, kind_end, _reservation) = match lp_utf16_bounded_scoped(
                ctx,
                bytes,
                at,
                1..=256,
                "f3d Design temporary UTF-16 text",
            ) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => continue,
                Err(error) => return Some(Err(error)),
            };
            if !kind.chars().all(|character| !character.is_control()) {
                continue;
            }
            let Some(tail_length) = paired_at.checked_sub(kind_end) else {
                continue;
            };
            let fixed_tail = matches!(tail_length, 72 | 76 | 77 | 78 | 82 | 87 | 88 | 104 | 110);
            if fixed_tail
                && parameter_scope_tail_length_is_valid(&kind, tail_length)
                && fixed_candidate
                    .replace((at, kind_end, tail_length, ScopeTailForm::Fixed))
                    .is_some()
            {
                fixed_ambiguous = true;
            }
            let named_tail_possible = (78..=590).contains(&tail_length)
                && tail_length.is_multiple_of(2)
                && (parameter_scope_tail_length_is_valid(&kind, tail_length) || tail_length == 78);
            let named_tail = if named_tail_possible {
                match named_parameter_scope_tail_is_valid(
                    ctx,
                    bytes,
                    kind_end,
                    paired_at,
                    tail_length,
                ) {
                    Ok(Some(valid)) => valid,
                    Ok(None) => false,
                    Err(error) => return Some(Err(error)),
                }
            } else {
                false
            };
            if named_tail
                && named_candidate
                    .replace((at, kind_end, tail_length, ScopeTailForm::Named))
                    .is_some()
            {
                named_ambiguous = true;
            }
        }
        let candidate = if named_ambiguous {
            None
        } else if named_candidate.is_some() {
            named_candidate
        } else if fixed_ambiguous {
            None
        } else {
            fixed_candidate
        };
        let (kind_at, kind_end, tail_length, tail_form) = candidate?;
        let (kind_text, confirmed_kind_end) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            kind_at,
            1..=256,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(decoded)) => decoded,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if confirmed_kind_end != kind_end {
            return None;
        }
        if let Err(error) = ctx.charge_retained(
            u64_from_index(kind_text.len()),
            "f3d Design scope kind storage",
        ) {
            return Some(Err(error));
        }
        let kind = scope::DesignFeatureKind::try_from(kind_text).ok()?;
        let reference_table_end = kind_at.checked_sub(4)?;
        let feature_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, kind_end)?)?;
        let history_state_id_offset = reference_table_end;
        let history_state_id = match View::u32_le_at(bytes, history_state_id_offset)? {
            u32::MAX => None,
            state_id => Some(i64::from(state_id)),
        };
        let previous_history_state_id_offset =
            match parameter_scope_previous_history_offset_for_form(
                kind.as_str(),
                tail_length,
                tail_form,
            ) {
                Some(offset) => Some(kind_end.checked_add(offset)?),
                None => None,
            };
        let previous_history_state_id = previous_history_state_id_offset.and_then(|offset| {
            match View::u32_le_at(bytes, offset)? {
                u32::MAX => None,
                state_id => Some(i64::from(state_id)),
            }
        });
        let mut reference_table = None;
        for count_at in start + 11..reference_table_end {
            let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
            if count == 0
                || count_at
                    .checked_add(4)?
                    .checked_add(count.checked_mul(11)?)?
                    != reference_table_end
            {
                continue;
            }
            let first = count_at.checked_add(4)?;

            let mut members = Vec::new();
            if let Err(error) =
                ctx.reserve_vec(&mut members, count, "f3d Design scope reference members")
            {
                return Some(Err(error));
            }

            let mut offsets = Vec::new();
            if let Err(error) =
                ctx.reserve_vec(&mut offsets, count, "f3d Design scope reference offsets")
            {
                return Some(Err(error));
            }
            for ordinal in 0..count {
                let marker = first.checked_add(ordinal.checked_mul(11)?)?;
                if bytes.get(marker) != Some(&1) || bytes.get(marker + 5..marker + 11)? != [0; 6] {
                    members.clear();
                    break;
                }
                members.push(View::u32_le_at(bytes, marker + 1)?);
                offsets.push(u64::try_from(marker + 1).ok()?);
            }
            if members.len() == count
                && reference_table
                    .replace((count_at, members, offsets))
                    .is_some()
            {
                return None;
            }
        }
        let reference_table = reference_table?;
        let (reference_count_at, reference_members, reference_member_offsets) = &reference_table;
        let surface_stitch_operation = if kind == scope::DesignFeatureKind::SurfaceStitch {
            exact_surface_stitch_operation(bytes, records, record_index, reference_members)
        } else {
            None
        };
        let surface_patch_boundaries = if kind == scope::DesignFeatureKind::SurfacePatch {
            match crate::design::decode::patch::surface_patch_boundaries(
                ctx,
                bytes,
                records,
                reference_members,
            ) {
                Ok(boundaries) => boundaries,
                Err(error) => return Some(Err(error)),
            }
        } else {
            Vec::new()
        };
        let base_flange_operation = if kind == scope::DesignFeatureKind::BaseFlange {
            exact_base_flange_operation(bytes, start, paired_at, reference_members)
        } else {
            None
        };
        let edge_flange_operation = if kind == scope::DesignFeatureKind::EdgeFlange {
            match exact_edge_flange_operation(
                ctx,
                bytes,
                start,
                paired_at,
                class_tag.as_str(),
                paired_class_tag,
                reference_members,
            ) {
                Ok(operation) => operation,
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let ruled_surface_operation = if kind == scope::DesignFeatureKind::SurfaceRuled {
            match exact_ruled_surface_operation(
                ctx,
                bytes,
                start,
                paired_at,
                *reference_count_at,
                reference_members,
            ) {
                Ok(operation) => operation,
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let family = design_feature_family(&kind);
        // A `Sketch` scope carries either the single entity-suffix reference form
        // or, when the stream's sketch entity headers use the `EntityGenesis`
        // form, the generic ordered reference table. Both parse here; the entity
        // binding in `decode_parameter_scopes` requires a unique suffix match.
        let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
            // The generic scope envelope is independently self-delimiting. An
            // unrecognized Extrude prologue therefore withholds only the typed
            // fields, not the scope and its ordered reference table.
            exact_extrude_prologue(
                bytes,
                start,
                paired_at,
                class_tag.as_str(),
                paired_class_tag,
                *reference_count_at,
                reference_members,
            )
        } else {
            None
        };
        let coil_discriminators = if family == Some(DesignFeatureFamily::Coil) {
            exact_coil_discriminators(bytes, start, paired_at, &kind, reference_members)
        } else {
            None
        };
        let coil_transform = if family == Some(DesignFeatureFamily::Coil) {
            exact_long_coil_transform(bytes, start, paired_at, &kind, reference_members)
        } else {
            None
        };
        let coil = if family == Some(DesignFeatureFamily::Coil) {
            Some(coil::DesignCoilScope {
                operation: coil_discriminators.as_ref().map(|fields| {
                    crate::records::identity::RecordedValue {
                        value: fields.operation,
                        offset: fields.operation_offset,
                    }
                }),
                extent: coil_discriminators
                    .as_ref()
                    .and_then(|fields| fields.extent),
                section: coil_discriminators
                    .as_ref()
                    .map(|fields| match fields.section_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.section,
                                offset,
                            },
                        ),
                        None => {
                            crate::records::identity::MaybeRecordedValue::Unlocated(fields.section)
                        }
                    }),
                section_placement: coil_discriminators.as_ref().map(|fields| {
                    match fields.section_placement_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.section_placement,
                                offset,
                            },
                        ),
                        None => crate::records::identity::MaybeRecordedValue::Unlocated(
                            fields.section_placement,
                        ),
                    }
                }),
                clockwise: coil_discriminators.as_ref().map(|fields| {
                    match fields.clockwise_offset {
                        Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                            crate::records::identity::RecordedValue {
                                value: fields.clockwise,
                                offset,
                            },
                        ),
                        None => crate::records::identity::MaybeRecordedValue::Unlocated(
                            fields.clockwise,
                        ),
                    }
                }),
                placement: None,
                transform: coil_transform,
            })
        } else {
            None
        };

        let mut located_references = Vec::new();
        if let Err(error) = ctx.reserve_vec(
            &mut located_references,
            reference_members.len(),
            "f3d Design scope located references",
        ) {
            return Some(Err(error));
        }
        located_references.extend(
            reference_members
                .iter()
                .copied()
                .zip(reference_member_offsets.iter().copied())
                .map(|(value, offset)| crate::records::identity::Located { value, offset }),
        );
        let mut scope = DesignParameterScope::try_new(scope::DesignParameterScopeDraft {
            id: String::new(),
            byte_offset,
            class_tag: class_tag.clone(),
            record_index,
            frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
            kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
            feature_ordinal,
            feature_ordinal_offset: u64::try_from(kind_end).ok()?,
            history_state_id,

            previous_history_state_id,
            previous_history_state_id_offset: previous_history_state_id_offset
                .and_then(|offset| u64::try_from(offset).ok())
                .filter(|&offset| offset != 0),
            reference_count_offset: u64::try_from(*reference_count_at).ok()?,
            reference_members: crate::records::identity::ReferenceRun::located(located_references),
            payload: match kind {
                scope::DesignFeatureKind::SurfaceStitch => {
                    scope::DesignScopePayload::SurfaceStitch(surface_stitch_operation?)
                }
                scope::DesignFeatureKind::SurfaceRuled => {
                    scope::DesignScopePayload::SurfaceRuled(ruled_surface_operation?)
                }
                kind => kind.try_into().ok()?,
            },
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
                .ok()?,
            paired_byte_offset: u64_from_index(paired_at),
        })
        .ok()?;
        if let Some(prologue) = extrude_prologue {
            {
                let construction = Some(scope::DesignExtrudeScope {
                    extrude_prologue: Some(prologue),
                    ..scope::DesignExtrudeScope::default()
                });
                if let scope::DesignScopePayloadMut::Extrude(slot)
                | scope::DesignScopePayloadMut::Extrusion(slot)
                | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
        }
        if let Some(coil) = coil {
            {
                let construction = Some(coil);
                if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
                | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
        }
        if !surface_patch_boundaries.is_empty() {
            {
                let construction = surface_patch_boundaries;
                if let scope::DesignScopePayloadMut::SurfacePatch(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
        }
        if let Some(operation) = base_flange_operation {
            {
                let construction = Some(scope::DesignBaseFlangeScope {
                    base_flange_operation: Some(operation),
                    ..scope::DesignBaseFlangeScope::default()
                });
                if let scope::DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
        }
        if let Some(operation) = edge_flange_operation {
            {
                let construction = Some(operation);
                if let scope::DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
        }
        Some(Ok(scope))
    })()
    .transpose()
}

fn named_parameter_scope_tail_is_valid(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    kind_end: usize,
    paired_at: usize,
    tail_length: usize,
) -> Result<Option<bool>, CodecError> {
    let Some(label_at) = kind_end.checked_add(8) else {
        return Ok(None);
    };
    let Some((label, label_end, _reservation)) = lp_utf16_bounded_scoped(
        ctx,
        bytes,
        label_at,
        0..=256,
        "f3d Design temporary UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    Ok((|| {
        let label_code_units = label.encode_utf16().count();
        if tail_length != 78usize.checked_add(label_code_units.checked_mul(2)?)?
            || label_end.checked_add(7)? != kind_end.checked_add(19 + label_code_units * 2)?
            || label.chars().any(char::is_control)
        {
            return Some(false);
        }
        let marker = kind_end.checked_add(19 + label_code_units.checked_mul(2)?)?;
        if marker.checked_add(59)? != paired_at || bytes.get(label_end..marker)? != [0; 7] {
            return Some(false);
        }
        let first_lane_value = View::u64_le_at(bytes, marker + 2)?;
        let second_lane_value = View::u64_le_at(bytes, marker + 34)?;
        let third_lane_value = View::u64_le_at(bytes, marker + 48)?;
        Some(
            bytes.get(kind_end + 4..kind_end + 8)? == [0; 4]
                && bytes.get(marker) == Some(&1)
                && bytes.get(marker + 1).is_some_and(|field_id| *field_id != 0)
                && matches!(first_lane_value, 0 | 1)
                && second_lane_value == first_lane_value
                && third_lane_value == first_lane_value
                && bytes.get(marker + 10..marker + 12)? == [0; 2]
                && View::u32_le_at(bytes, marker + 12)? > 0
                && View::u32_le_at(bytes, marker + 16)? == 0xfc
                && View::f64_le_at(bytes, marker + 20)?.is_finite()
                && View::u32_le_at(bytes, marker + 28)? == 0xfc
                && bytes.get(marker + 32) == Some(&1)
                && bytes
                    .get(marker + 33)
                    .is_some_and(|field_id| *field_id != 0)
                && bytes.get(marker + 42..marker + 46)? == [0, 1, 0, 0]
                && bytes.get(marker + 46) == Some(&1)
                && bytes
                    .get(marker + 47)
                    .is_some_and(|field_id| *field_id != 0)
                && bytes.get(marker + 56..marker + 59)? == [0; 3],
        )
    })())
}

pub(crate) fn parameter_scope_payload_length(scope: &DesignParameterScope) -> Option<u64> {
    let kind_bytes = u64::try_from(scope.kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    scope.frame_length().checked_sub(kind_bytes)
}

#[cfg(test)]
mod tests;
