// SPDX-License-Identifier: Apache-2.0
//! Exact assembly alignment scopes.

use super::assembly_operand_frames::exact_as_built_operand_frames;
use super::assembly_operand_frames::exact_assembly_operand_frames;
use super::assembly_operand_paths::exact_assembly_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_383_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::legacy_operand_paths::CLASS_383_OWNER_REFERENCE_ORDINALS;
use super::legacy_operand_paths::CLASS_388_OWNER_REFERENCE_ORDINALS;
use crate::design::decode::assembly::exact_legacy_as_built_421_alignment;
use crate::design::decode::assembly::exact_legacy_as_built_421_solved_frame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::assembly;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

struct AsBuiltAlignmentDraft {
    paths: [DesignAssemblyOperandPath; 2],
    frames: Option<[DesignAssemblyOperandFrame; 2]>,
}

impl TryFrom<AsBuiltAlignmentDraft> for assembly::DesignAssemblyAlignmentForm {
    type Error = &'static str;

    fn try_from(draft: AsBuiltAlignmentDraft) -> Result<Self, Self::Error> {
        Ok(Self::qualified(
            draft.frames.ok_or("as-built operand_frames are required")?,
            draft
                .paths
                .map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }),
        ))
    }
}

pub(super) fn exact_assembly_alignment(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    use assembly::DesignAssemblyAlignmentForm;
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Assemble) {
        return Ok(None);
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let matching = |owner: &&DesignParameterOwner| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == scope.record_index
    };
    let lane_count = parameter_owners.iter().filter(matching).count();

    let mut lanes = Vec::new();
    ctx.reserve_vec(&mut lanes, lane_count, "f3d assembly alignment lanes")?;
    lanes.extend(parameter_owners.iter().filter(matching));
    ctx.stable_sort_by(
        &mut lanes[..],
        |left, right| {
            let left_key = {
                let owner = left;
                owner.local_ordinal()
            };
            let right_key = {
                let owner = right;
                owner.local_ordinal()
            };
            left_key.cmp(&right_key)
        },
        |_| 0,
        "sort f3d design assembly_alignment 1",
    )?;
    (|| -> Option<Result<DesignAssemblyAlignment, CodecError>> {
        if lanes
            .iter()
            .enumerate()
            .any(|(ordinal, owner)| u32::try_from(ordinal) != Ok(owner.local_ordinal()))
        {
            return None;
        }
        let as_built_421 = crate::design::assembly::legacy_as_built_421_generation(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .is_some();
        let legacy_class_383 = crate::design::assembly::legacy_class_383_258_scope(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
        let generation = crate::design::assembly::AssemblyScopeGeneration::new(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
        let legacy_class_388 = matches!(
            generation.operand_frame_variant(),
            Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
        );
        if legacy_class_388 {
            exact_legacy_class_388_scope(bytes, scope)?;
        }

        if as_built_421 {
            let exact = exact_legacy_as_built_421_alignment(bytes, scope, &lanes)?;
            let form = match exact_legacy_as_built_421_solved_frame(bytes, records, scope) {
                Some(solved_frame) => DesignAssemblyAlignmentForm::SolvedOnly {
                    solved_frame,
                    limits: Some(exact.limits),
                },
                None => DesignAssemblyAlignmentForm::LimitsOnly {
                    limits: exact.limits,
                },
            };
            return DesignAssemblyAlignment::try_new(
                exact.angle,
                exact.offset,
                exact.owners,
                Some(form),
            )
            .ok()
            .map(Ok);
        }
        let (angle, offset, owners) = {
            if matches!(scope.frame_length(), 671 | 744 | 748)
                && generation.operand_frame_variant().is_none()
            {
                return None;
            }
            let (alignment_start, alignment_end) = generation.alignment_lane_bounds(lanes.len())?;
            let alignment_lanes = lanes.get(alignment_start..alignment_end)?;
            let (angle, offset) = match alignment_lanes {
                [angle, offset_x, offset_y, offset_z] => (
                    angle.evaluated_value().get(),
                    [
                        offset_x.evaluated_value().get(),
                        offset_y.evaluated_value().get(),
                        offset_z.evaluated_value().get(),
                    ],
                ),
                [angle, axial_offset] => (
                    angle.evaluated_value().get(),
                    [0.0, 0.0, axial_offset.evaluated_value().get()],
                ),
                _ => return None,
            };
            // Both supported alignment lanes use one fixed four-owner allocation.
            let mut owners = match ctx.collection_vec(4, "collect F3D assembly alignment owners") {
                Ok(values) => values,
                Err(error) => return Some(Err(error)),
            };
            for owner in alignment_lanes {
                owners.push(crate::records::identity::Located {
                    value: owner.record_index(),
                    offset: owner.evaluated_value_offset(),
                });
            }
            if legacy_class_388 {
                let owner_reference_order_matches = CLASS_388_OWNER_REFERENCE_ORDINALS
                    .into_iter()
                    .zip(lanes.iter())
                    .all(|(scope_ordinal, owner)| {
                        scope.reference_members().values().nth(scope_ordinal)
                            == Some(&owner.record_index())
                    });
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "282" || owner.frame_length() != 103)
                    || !owner_reference_order_matches
                    || !scope
                        .reference_members()
                        .values()
                        .skip(4)
                        .take(4)
                        .eq(owners.iter().map(|owner| &owner.value))
                {
                    return None;
                }
            } else if legacy_class_383 {
                let owner_reference_order_matches = CLASS_383_OWNER_REFERENCE_ORDINALS
                    .into_iter()
                    .zip(lanes.iter())
                    .all(|(scope_ordinal, owner)| {
                        scope.reference_members().values().nth(scope_ordinal)
                            == Some(&owner.record_index())
                    });
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "284" || owner.frame_length() != 103)
                    || !owner_reference_order_matches
                    || !scope
                        .reference_members()
                        .values()
                        .skip(8)
                        .take(4)
                        .eq(owners.iter().map(|owner| &owner.value))
                {
                    return None;
                }
            } else if crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) {
                if lanes
                    .iter()
                    .any(|owner| owner.class_tag().as_str() != "289" || owner.frame_length() != 103)
                    || (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + owners.len())
                                .is_some_and(|values| {
                                    values.eq(owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        != 1
                {
                    return None;
                }
            } else if !scope
                .reference_members()
                .values()
                .rev()
                .take(owners.len())
                .eq(owners.iter().map(|owner| &owner.value).rev())
            {
                return None;
            }
            (angle, offset, owners)
        };
        let form = if scope.kind() == scope::DesignFeatureKind::AsBuilt {
            let paths = match exact_assembly_operand_paths(ctx, bytes, records, scope) {
                Ok(paths) => paths,
                Err(error) => return Some(Err(error)),
            };
            paths
                .map(|paths| {
                    DesignAssemblyAlignmentForm::try_from(AsBuiltAlignmentDraft {
                        frames: exact_as_built_operand_frames(bytes, &paths),
                        paths,
                    })
                })
                .transpose()
                .ok()?
        } else if let Some(frames) = exact_assembly_operand_frames(bytes, scope) {
            let qualifiers = if legacy_class_383 {
                let paths = match exact_legacy_class_383_operand_paths(ctx, bytes, records, scope, &frames) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }))
            } else if legacy_class_388 {
                let paths = match exact_legacy_class_388_operand_paths(ctx, bytes, records, scope) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| {
                    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                })
            } else if crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) {
                let direct = match super::assembly_carrier_paths::exact_variable_reference_operand_qualifiers(
                    ctx, bytes, records, scope, &frames,
                ) {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                };
                if direct.is_some() {
                    direct
                } else {
                    let paths = match exact_assembly_operand_paths(ctx, bytes, records, scope) {
                        Ok(paths) => paths,
                        Err(error) => return Some(Err(error)),
                    };
                    paths.map(|paths| {
                        paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                    })
                }
            } else {
                let paths = match exact_assembly_operand_paths(ctx, bytes, records, scope) {
                    Ok(paths) => paths,
                    Err(error) => return Some(Err(error)),
                };
                paths.map(|paths| {
                    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
                })
            };
            Some(match qualifiers {
                Some(qualifiers) => DesignAssemblyAlignmentForm::qualified(frames, qualifiers),
                None => DesignAssemblyAlignmentForm::Frames { frames },
            })
        } else {
            None
        };
        DesignAssemblyAlignment::try_new(angle, offset, owners, form)
            .ok()
            .map(Ok)
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
