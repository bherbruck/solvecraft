// SPDX-License-Identifier: Apache-2.0
//! Resolve extrude profile selections against sketch regions.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::{entity_selection_matches_curve, parse_sketch_profile};
use crate::design::geometry::{
    arrangement_region_containing_points, historical_member_points_in_state, point_in_polygon,
    point_on_sketch_entity, point_segment_distance, profile_loops_are_independent,
    project_to_sketch, region_containing_points,
};
use crate::ids::native_stream;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    sketch_geometry::SketchCurveIdentity,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelationOperand,
    topology::{
        construction::DesignConstructionOperandGroup,
        entity_selection::DesignEntitySelectionOperand,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
        sketch_profile::DesignSketchProfileOperand,
        sketch_profile::DesignSketchProfileRegionMember,
    },
};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
    pub(crate) arrangement_budget: &'a WorkBudget<'a>,
    pub(crate) ctx: &'a DecodeContext<'a>,
}

#[derive(Clone, Copy)]
pub(in crate::design) struct ScopedExtrudeProfileResolution<'a> {
    entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &'a [crate::history_records::AsmHistory],
    linear_tolerance: f64,
    angular_tolerance: f64,
    arrangement_budget: &'a WorkBudget<'a>,
    ctx: &'a DecodeContext<'a>,
}

impl<'a> ExtrudeProfileResolution<'a> {
    pub(super) fn scoped(
        self,
        histories: &'a [crate::history_records::AsmHistory],
    ) -> ScopedExtrudeProfileResolution<'a> {
        ScopedExtrudeProfileResolution {
            entities: self.entities,
            spatial_entities: self.spatial_entities,
            histories,
            linear_tolerance: self.linear_tolerance,
            angular_tolerance: self.angular_tolerance,
            arrangement_budget: self.arrangement_budget,
            ctx: self.ctx,
        }
    }
}

fn histories_for_scope<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [crate::history_records::AsmHistory],
) -> &'a [crate::history_records::AsmHistory] {
    if scope_histories.contains_key(scope_id) {
        crate::history::bound_scope_history(scope_id, scope_histories, histories)
            .map_or(&[], std::slice::from_ref)
    } else {
        histories
    }
}

/// Native and neutral arenas required to resolve curve selections in sketches.
pub(crate) struct SketchCurveSelectionResolution<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Counted construction-operand groups.
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    /// Nested entity-selection operands.
    pub(crate) operands: &'a [DesignEntitySelectionOperand],
    /// Projected Sketch placement carriers.
    pub(crate) placements: &'a [DesignSketchPlacement],
    /// Persistent Sketch curve identities.
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    /// Neutral planar Sketches.
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    /// Neutral planar Sketch entities.
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    /// Neutral model-space Sketches for non-planar sketch carriers.
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    /// Neutral model-space entities for non-planar sketch carriers.
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

#[derive(Clone, Copy)]
struct EntitySelectionPathResolution<'a> {
    operands: &'a [DesignEntitySelectionOperand],
    placements: &'a [DesignSketchPlacement],
    curve_identities: &'a [SketchCurveIdentity],
    sketches: &'a [cadmpeg_ir::sketches::Sketch],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

impl<'a> SketchCurveSelectionResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.operands,
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

/// Bind exact Sweep sketch-profile and sole-curve path carriers.
pub(crate) fn bind_sweep_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef};
    let SketchCurveSelectionResolution {
        scopes,
        groups,
        operands,
        placements,
        curve_identities,
        sketches,
        sketch_entities,
        ..
    } = resolution;
    let path_resolution = resolution.path_resolution();
    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(native_ref) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };
                    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
                    let Some(scope) = matching_scopes.next() else {
                        break 'feature_edit;
                    };
                    if matching_scopes.next().is_some() {
                        break 'feature_edit;
                    }
                    let Some(stream) = native_stream(&scope.id) else {
                        break 'feature_edit;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::Sweep {
                        shape,
                        path,
                        guide_rail,
                        ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    {
                        let section = &mut *shape;
                        if let (Some(PlanarProfileRef::Native(group_id)), Some(profile_operand)) =
                            (section.referenced_profile(), scope.sweep_profile())
                        {
                            let group_id = group_id.as_str();
                            let group_matches = {
                                let mut matching_groups = groups.iter().filter(|group| {
                                    group.id == group_id
                                        && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::PROFILE
                                        && group
                                            .members()
                                            .iter()
                                            .map(|member| member.value)
                                            .eq([profile_operand.record_index])
                                        && native_stream(&group.id) == Some(stream)
                                });
                                matches!(
                                    (matching_groups.next(), matching_groups.next()),
                                    (Some(_), None)
                                )
                            };
                            if group_matches {
                                let mut candidates = placements.iter().filter(|placement| {
                                    native_stream(&placement.id) == Some(stream)
                                        && placement.entity_id.suffix()
                                            == profile_operand.entity_id.suffix()
                                });
                                if let (Some(placement), None) =
                                    (candidates.next(), candidates.next())
                                {
                                    let sketch =
                                        crate::design::identity::neutral_sketch_id(ctx, placement)?;
                                    if sketches.iter().any(|candidate| candidate.id == sketch) {
                                        section.set_referenced_profile((sketch).into());
                                    }
                                }
                            }
                        }
                        if let Some(PlanarProfileRef::Native(group_id)) =
                            section.referenced_profile()
                        {
                            let group_id = group_id.as_str();
                            let resolved = (|| -> Result<Option<_>, CodecError> {
                                let mut matching_groups = groups.iter().filter(|group| {
                                    group.id == group_id
                                        && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::PROFILE
                                        && group.members().len() == 1
                                        && native_stream(&group.id) == Some(stream)
                                });
                                let group = or_none!(matching_groups.next());
                                if matching_groups.next().is_some() {
                                    return Ok(None);
                                }
                                let mut matching_operands = operands.iter().filter(|operand| {
                                    operand.scope_record_index == scope.record_index
                                        && operand.group_record_index == group.record_index
                                        && operand.group_member_ordinal == 0
                                        && operand.record_index() == group.members()[0].value
                                        && native_stream(&operand.id) == Some(stream)
                                });
                                let operand = or_none!(matching_operands.next());
                                if matching_operands.next().is_some() {
                                    return Ok(None);
                                }
                                let mut matching_placements =
                                    placements.iter().filter(|placement| {
                                        native_stream(&placement.id) == Some(stream)
                                            && placement.entity_id.suffix()
                                                == operand.primary_identity
                                    });
                                let placement = or_none!(matching_placements.next());
                                if matching_placements.next().is_some() {
                                    return Ok(None);
                                }
                                let sketch =
                                    crate::design::identity::neutral_sketch_id(ctx, placement)?;
                                if !sketches.iter().any(|candidate| candidate.id == sketch) {
                                    return Ok(None);
                                }
                                let owner_reference =
                                    or_none!(u32::try_from(operand.primary_identity).ok());
                                let mut matching_curves = curve_identities.iter().filter(|curve| {
                                    native_stream(&curve.id) == Some(stream)
                                        && curve.owner_reference == Some(owner_reference)
                                        && entity_selection_matches_curve(operand, curve)
                                });
                                let curve = or_none!(matching_curves.next());
                                if matching_curves.next().is_some() {
                                    return Ok(None);
                                }
                                let selected = crate::design::identity::neutral_sketch_curve_id(
                                    ctx,
                                    &sketch,
                                    curve.primary_id.get(),
                                    curve.secondary_id,
                                )?;
                                Ok(sketch_entities
                                    .iter()
                                    .any(|entity| {
                                        entity.sketch == sketch && entity.id() == &selected
                                    })
                                    .then_some((sketch, selected)))
                            })()?;
                            if let Some((sketch, selected)) = resolved {
                                let profile =
                                    PlanarProfileRef::sketch_entities(sketch, vec![selected], ctx)?
                                        .map_err(CodecError::malformed)?;
                                section.set_referenced_profile(profile);
                            }
                        }
                        let resolve_path = |path: &PathRef| -> Result<Option<PathRef>, CodecError> {
                            let PathRef::Native(group_id) = path else {
                                return Ok(None);
                            };
                            let mut matching_groups = groups.iter().filter(|group| {
                                group.id == *group_id
                                    && group.scope_record_index == scope.record_index
                                    && group.role() == DesignOperandRole::ROLE_0X5
                                    && native_stream(&group.id) == Some(stream)
                            });
                            let Some(group) = matching_groups.next() else {
                                return Ok(None);
                            };
                            if matching_groups.next().is_some() || group.members().len() != 1 {
                                return Ok(None);
                            }
                            resolve_entity_selection_path(group, &path_resolution, ctx)
                        };
                        if let Some(path) = path {
                            if let Some(resolved) = resolve_path(path)? {
                                *path = resolved;
                            }
                        }
                        if let Some(guide_rail) = guide_rail {
                            if let Some(resolved) = resolve_path(&guide_rail.path)? {
                                guide_rail.path = resolved;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve `SplitFace` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_split_face_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let FeatureDefinition::Operation(FeatureOperation::SplitFace { tool, .. }) =
                        definition
                    else {
                        break 'feature_edit;
                    };
                    let SplitFaceTool::Path(PathRef::Native(group_id)) = tool else {
                        break 'feature_edit;
                    };
                    let mut matching_groups = resolution.groups.iter().filter(|group| {
                        group.id == *group_id
                            && group.role() == DesignOperandRole::ROLE_0X21
                            && !group.members().is_empty()
                    });
                    let Some(group) = matching_groups.next() else {
                        break 'feature_edit;
                    };
                    if matching_groups.next().is_some() {
                        break 'feature_edit;
                    }
                    if let Some(path) = resolve_entity_selection_path(group, &path_resolution, ctx)?
                    {
                        *tool = SplitFaceTool::Path(path);
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve `SurfaceTrim` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_surface_trim_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                        tool, ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    let PathRef::Native(group_id) = tool else {
                        break 'feature_edit;
                    };
                    let mut matching_groups = resolution.groups.iter().filter(|group| {
                        group.id == *group_id
                            && group.role() == DesignOperandRole::ROLE_0X21
                            && !group.members().is_empty()
                    });
                    let Some(group) = matching_groups.next() else {
                        break 'feature_edit;
                    };
                    if matching_groups.next().is_some() {
                        break 'feature_edit;
                    }
                    if let Some(path) = resolve_entity_selection_path(group, &path_resolution, ctx)?
                    {
                        *tool = path;
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(scope) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };
                    let Some(scope) = scopes.iter().find(|candidate| candidate.id == scope) else {
                        break 'feature_edit;
                    };
                    let scoped_histories = histories_for_scope(
                        &scope.id,
                        resolution.scope_histories,
                        resolution.histories,
                    );
                    let scoped_resolution = resolution.scoped(scoped_histories);
                    let effective_previous_history_state_id =
                        crate::history::effective_scope_previous_history_state_id(
                            scope,
                            scoped_histories,
                        );
                    let mut matching_groups = Vec::new();
                    for group in groups.iter().filter(|group| {
                        native_stream(&group.id) == native_stream(&scope.id)
                            && group.scope_record_index == scope.record_index
                    }) {
                        (resolution.ctx).push_vec(
                            &mut matching_groups,
                            group,
                            "f3d extrude matching selection group",
                        )?;
                    }
                    resolution.ctx.stable_sort_by(
                        &mut matching_groups[..],
                        |left, right| {
                            let left_key = {
                                let group = left;
                                group.scope_reference_ordinal
                            };
                            let right_key = {
                                let group = right;
                                group.scope_reference_ordinal
                            };
                            left_key.cmp(&right_key)
                        },
                        |_| 0,
                        "sort f3d design profile_select 1",
                    )?;
                    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                        definition
                    else {
                        break 'feature_edit;
                    };
                    if let ProfileRef::Planar(PlanarProfileRef::Native(native)) = profile {
                        let mut entity_groups = curve_resolution.groups.iter().filter(|group| {
                            group.id == *native
                                && native_stream(&group.id) == native_stream(&scope.id)
                                && group.scope_record_index == scope.record_index
                        });
                        if let (Some(group), None) = (entity_groups.next(), entity_groups.next()) {
                            if let Some(selection) = resolve_entity_selection_profile(
                                group,
                                &curve_resolution.path_resolution(),
                                resolution.ctx,
                            )? {
                                *profile = selection;
                                break 'feature_edit;
                            }
                        }
                        if let Some(selection) = historical_face_profile_selection(
                            &matching_groups,
                            members,
                            effective_previous_history_state_id,
                            &feature.id,
                            scoped_histories,
                            resolution.ctx,
                        )? {
                            *profile = ProfileRef::Planar(selection);
                        }
                        break 'feature_edit;
                    }
                    let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) = profile else {
                        break 'feature_edit;
                    };
                    let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id)
                    else {
                        if matching_groups.is_empty() {
                            break 'feature_edit;
                        }
                        let spatial_id_parts = sketch_id.as_str().split_once("f3d:model:sketch#");
                        if let Some(spatial_sketch) =
                            resolution.spatial_sketches.iter().find(|candidate| {
                                match spatial_id_parts {
                                    Some((before, after)) => {
                                        candidate.id.as_str().strip_prefix(before).and_then(
                                            |rest| rest.strip_prefix("f3d:model:spatial-sketch#"),
                                        ) == Some(after)
                                    }
                                    None => candidate.id.as_str() == sketch_id.as_str(),
                                }
                            })
                        {
                            let mut selections = Vec::new();
                            for group in &matching_groups {
                                let selection = resolved_spatial_extrude_profile_selection(
                                    group,
                                    members,
                                    spatial_sketch,
                                    scoped_resolution.spatial_entities,
                                    scoped_resolution,
                                    scope.history_state_id(),
                                    effective_previous_history_state_id,
                                )?;
                                (scoped_resolution.ctx).push_vec(
                                    &mut selections,
                                    selection,
                                    "f3d spatial profile selection",
                                )?;
                            }
                            let mut indices = Vec::new();
                            let mut all_resolved = true;
                            for selection in &selections {
                                let Some(index) = selection else {
                                    all_resolved = false;
                                    break;
                                };
                                if !indices.contains(index) {
                                    (resolution.ctx).push_vec(
                                        &mut indices,
                                        *index,
                                        "f3d spatial extrude profile index",
                                    )?;
                                }
                            }
                            if all_resolved {
                                let sketch_id = spatial_sketch.id.try_clone_for_decode(
                                    resolution.ctx,
                                    "f3d profile spatial sketch id",
                                )?;
                                *profile = match ProfileRef::spatial_sketch_profiles(
                                    sketch_id,
                                    indices,
                                    resolution.ctx,
                                )? {
                                    Ok(profile) => profile,
                                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                        (resolution.ctx).copy_retained_text(
                                            &scope.id,
                                            "f3d spatial extrude fallback scope id",
                                        )?,
                                    )),
                                };
                            } else {
                                let mut group_ids = Vec::new();
                                for group in &matching_groups {
                                    let id = (resolution.ctx).copy_retained_text(
                                        &group.id,
                                        "f3d spatial extrude selection group id",
                                    )?;
                                    (resolution.ctx).push_vec(
                                        &mut group_ids,
                                        id,
                                        "f3d spatial extrude selection group",
                                    )?;
                                }
                                let sketch_id = spatial_sketch.id.try_clone_for_decode(
                                    resolution.ctx,
                                    "f3d profile spatial sketch id",
                                )?;
                                *profile = match ProfileRef::spatial_sketch_selection(
                                    sketch_id,
                                    group_ids,
                                    resolution.ctx,
                                )? {
                                    Ok(profile) => profile,
                                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                        (resolution.ctx).copy_retained_text(
                                            &scope.id,
                                            "f3d spatial extrude fallback scope id",
                                        )?,
                                    )),
                                };
                            }
                            break 'feature_edit;
                        }
                        let id = match matching_groups.as_slice() {
                            [group] => &group.id,
                            _ => &scope.id,
                        };
                        *profile = ProfileRef::Planar(PlanarProfileRef::Native(
                            (resolution.ctx)
                                .copy_retained_text(id, "f3d extrude unresolved selection id")?,
                        ));
                        break 'feature_edit;
                    };
                    if let (Some(profile_operand), Some(stream)) =
                        (scope.extrude_profile(), native_stream(&scope.id))
                    {
                        if let Some(profiles) = resolved_sketch_profile_regions(
                            stream,
                            profile_operand,
                            sketch,
                            curve_resolution.curve_identities,
                            curve_resolution.sketch_entities,
                            resolution.ctx,
                        )? {
                            let sketch_id = (sketch_id)
                                .try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?;
                            *profile = ProfileRef::Planar(match PlanarProfileRef::sketch_profiles(
                                sketch_id,
                                profiles,
                                resolution.ctx,
                            )? {
                                Ok(profile) => profile,
                                Err(_) => {
                                    PlanarProfileRef::Native((resolution.ctx).copy_retained_text(
                                        &scope.id,
                                        "f3d extrude profile fallback scope id",
                                    )?)
                                }
                            });
                            break 'feature_edit;
                        }
                    }
                    if matching_groups.is_empty() {
                        break 'feature_edit;
                    }
                    let mut selections = Vec::new();
                    for group in &matching_groups {
                        let selection = resolved_extrude_profile_selection(
                            sketch_id,
                            group,
                            members,
                            sketch,
                            scoped_resolution,
                            scope.history_state_id(),
                            effective_previous_history_state_id,
                        )?;
                        (resolution.ctx).push_vec(
                            &mut selections,
                            selection,
                            "f3d extrude resolved selection",
                        )?;
                    }
                    *profile = if let Some(merged) =
                        merge_resolved_profile_selections(sketch_id, &selections, resolution.ctx)?
                    {
                        merged
                    } else {
                        let mut group_ids = Vec::new();
                        for group in &matching_groups {
                            let id = (resolution.ctx)
                                .copy_retained_text(&group.id, "f3d extrude fallback group id")?;
                            (resolution.ctx).push_vec(
                                &mut group_ids,
                                id,
                                "f3d extrude fallback group",
                            )?;
                        }
                        let sketch_id = (sketch_id)
                            .try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?;
                        let fallback = match PlanarProfileRef::sketch_selection(
                            sketch_id,
                            group_ids,
                            resolution.ctx,
                        )? {
                            Ok(profile) => profile,
                            Err(_) => {
                                PlanarProfileRef::Native((resolution.ctx).copy_retained_text(
                                    &scope.id,
                                    "f3d extrude fallback scope id",
                                )?)
                            }
                        };
                        ProfileRef::Planar(fallback)
                    };
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

fn resolve_entity_selection_profile(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::{PathRef, PlanarProfileRef, ProfileRef};

    if group.role() != DesignOperandRole::PROFILE {
        return Ok(None);
    }
    let Some(path) = resolve_entity_selection_path(group, resolution, ctx)? else {
        return Ok(None);
    };
    match path {
        PathRef::SketchCurves { sketch, curves } => {
            let Some(source) = resolution
                .sketches
                .iter()
                .find(|source| source.id == sketch)
            else {
                return Ok(None);
            };
            let mut selected_profiles = Vec::new();
            let mut has_unprofiled_entity = false;
            for curve in &curves {
                let mut matches = source
                    .profiles
                    .iter()
                    .enumerate()
                    .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == *curve));
                let Some((profile_index, _)) = matches.next() else {
                    has_unprofiled_entity = true;
                    continue;
                };
                if matches.next().is_some() {
                    return Ok(None);
                }
                let Ok(profile_index) = u32::try_from(profile_index) else {
                    return Ok(None);
                };
                if !selected_profiles.contains(&profile_index) {
                    ctx.push_vec(
                        &mut selected_profiles,
                        profile_index,
                        "f3d entity path selected planar profile",
                    )?;
                }
            }
            if has_unprofiled_entity {
                Ok(Some(ProfileRef::Planar(PlanarProfileRef::SketchEntities {
                    sketch,
                    entities: curves,
                })))
            } else {
                Ok(
                    PlanarProfileRef::sketch_profiles(sketch, selected_profiles, ctx)?
                        .ok()
                        .map(ProfileRef::Planar),
                )
            }
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let Some(source) = resolution
                .spatial_sketches
                .iter()
                .find(|source| source.id == sketch)
            else {
                return Ok(None);
            };
            let mut profiles = Vec::new();
            for curve in &curves {
                let mut matches = source.profiles.iter().enumerate().filter(|(_, profile)| {
                    profile.boundary().iter().any(|use_| use_.entity == *curve)
                });
                let Some((index, _)) = matches.next() else {
                    return Ok(None);
                };
                if matches.next().is_some() {
                    return Ok(None);
                }
                let Ok(index) = u32::try_from(index) else {
                    return Ok(None);
                };
                if !profiles.contains(&index) {
                    ctx.push_vec(
                        &mut profiles,
                        index,
                        "f3d entity path selected spatial profile",
                    )?;
                }
            }
            if profiles.is_empty() {
                return Ok(None);
            }
            Ok(ProfileRef::spatial_sketch_profiles(sketch, profiles, ctx)?.ok())
        }
        _ => Ok(None),
    }
}

fn historical_face_profile_selection(
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scoped_histories: &[crate::history_records::AsmHistory],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PlanarProfileRef>, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    let mut states = scoped_histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let Some(topology) = states.next().and_then(|state| state.topology()) else {
        return Ok(None);
    };
    if states.next().is_some() {
        return Ok(None);
    }
    let Some(stream) = groups.first().and_then(|group| native_stream(&group.id)) else {
        return Ok(None);
    };
    let mut selected_faces = Vec::new();
    for group in groups {
        if native_stream(&group.id) != Some(stream) {
            return Ok(None);
        }
        let mut group_members = Vec::new();
        for member in members.iter().filter(|member| {
            native_stream(&member.id) == Some(stream)
                && member.group_record_index == group.record_index
        }) {
            ctx.push_vec(
                &mut group_members,
                member,
                "f3d historical profile group member",
            )?;
        }
        ctx.stable_sort_by(
            &mut group_members[..],
            |left, right| {
                let left_key = {
                    let member = left;
                    {
                        member.group_member_ordinal
                    }
                };
                let right_key = {
                    let member = right;
                    {
                        member.group_member_ordinal
                    }
                };
                left_key.cmp(&right_key)
            },
            |_| 0,
            "sort f3d design profile_select 2",
        )?;
        if group_members.len() != group.members().len()
            || group_members
                .iter()
                .zip(group.members())
                .any(|(member, record_index)| member.record_index() != record_index.value)
        {
            return Ok(None);
        }
        let mut candidates = None::<HashSet<i64>>;
        for member in group_members {
            let (kind, entity_ref) = match &member.historical {
                Some(binding) => {
                    if !binding.state_ids.is_empty()
                        && !binding.state_ids.contains(&previous_state_id)
                    {
                        return Ok(None);
                    }
                    (Some(binding.kind), binding.entity_ref)
                }
                None => {
                    let Ok(entity_ref) = i64::try_from(member.local_id) else {
                        return Ok(None);
                    };
                    (None, entity_ref)
                }
            };
            let member_faces = historical_profile_face_candidates(kind, entity_ref, topology, ctx)?;
            if member_faces.is_empty() {
                return Ok(None);
            }
            candidates = Some(match candidates {
                None => member_faces,
                Some(mut candidates) => {
                    candidates.retain(|face| member_faces.contains(face));
                    candidates
                }
            });
        }
        let Some(candidates) = candidates else {
            return Ok(None);
        };
        let mut candidates = candidates.into_iter();
        let Some(face) = candidates.next() else {
            return Ok(None);
        };
        if candidates.next().is_some() {
            return Ok(None);
        }
        if !selected_faces.contains(&face) {
            ctx.push_vec(
                &mut selected_faces,
                face,
                "f3d historical profile selected face",
            )?;
        }
    }
    if selected_faces.is_empty() {
        return Ok(None);
    }
    let feature_key = crate::design::identity::identity_key(feature_id.as_str())?;
    let mut face_ids = Vec::new();
    for face in selected_faces {
        let id = crate::design::identity::history_input_face_id(
            ctx,
            &crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id)?,
            face,
            "f3d historical face identifier",
        )?;
        ctx.push_vec(&mut face_ids, id, "f3d historical profile face id")?;
    }
    let mut group_ids = Vec::new();
    for group in groups {
        let id = ctx.copy_retained_text(&group.id, "f3d historical profile group id")?;
        ctx.push_vec(&mut group_ids, id, "f3d historical profile group id entry")?;
    }
    Ok(PlanarProfileRef::historical_faces(
        crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?,
        face_ids,
        group_ids,
        ctx,
    )?
    .ok())
}

fn historical_profile_face_candidates(
    kind: Option<crate::records::topology::body_recipe::AsmHistoricalEntityKind>,
    entity_ref: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<HashSet<i64>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let all_kinds = [
        AsmHistoricalEntityKind::Face,
        AsmHistoricalEntityKind::Loop,
        AsmHistoricalEntityKind::Coedge,
        AsmHistoricalEntityKind::Edge,
        AsmHistoricalEntityKind::Pcurve,
        AsmHistoricalEntityKind::Curve,
        AsmHistoricalEntityKind::Vertex,
        AsmHistoricalEntityKind::Point,
        AsmHistoricalEntityKind::Surface,
    ];
    let kinds = kind
        .iter()
        .chain(all_kinds.iter().filter(|_| kind.is_none()));
    let loop_faces = |loop_ref| {
        let mut faces = HashSet::new();
        for relation in topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&loop_ref))
        {
            ctx.insert_hash_set(
                &mut faces,
                relation.owner_ref,
                "f3d historical loop face candidate",
            )?;
        }
        Ok::<_, CodecError>(faces)
    };
    let coedge_faces = |coedge_ref| {
        let mut faces = HashSet::new();
        for coedge in topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == coedge_ref)
        {
            for face in loop_faces(coedge.owner_loop)? {
                ctx.insert_hash_set(&mut faces, face, "f3d historical coedge face candidate")?;
            }
        }
        Ok::<_, CodecError>(faces)
    };
    let edge_faces = |edge_ref| {
        let mut faces = HashSet::new();
        for coedge in topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.edge == edge_ref)
        {
            for face in loop_faces(coedge.owner_loop)? {
                ctx.insert_hash_set(&mut faces, face, "f3d historical edge face candidate")?;
            }
        }
        Ok::<_, CodecError>(faces)
    };
    let mut faces = HashSet::new();
    for kind in kinds {
        match kind {
            AsmHistoricalEntityKind::Face => {
                if topology.faces.contains(&entity_ref) {
                    ctx.insert_hash_set(
                        &mut faces,
                        entity_ref,
                        "f3d historical profile face candidate",
                    )?;
                }
            }
            AsmHistoricalEntityKind::Loop => {
                for face in loop_faces(entity_ref)? {
                    ctx.insert_hash_set(&mut faces, face, "f3d historical profile face candidate")?;
                }
            }
            AsmHistoricalEntityKind::Coedge => {
                for face in coedge_faces(entity_ref)? {
                    ctx.insert_hash_set(&mut faces, face, "f3d historical profile face candidate")?;
                }
            }
            AsmHistoricalEntityKind::Edge => {
                for face in edge_faces(entity_ref)? {
                    ctx.insert_hash_set(&mut faces, face, "f3d historical profile face candidate")?;
                }
            }
            AsmHistoricalEntityKind::Pcurve => {
                for binding in topology
                    .coedge_pcurves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    for face in coedge_faces(binding.entity)? {
                        ctx.insert_hash_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )?;
                    }
                }
            }
            AsmHistoricalEntityKind::Curve => {
                for binding in topology
                    .edge_curves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    for face in edge_faces(binding.entity)? {
                        ctx.insert_hash_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )?;
                    }
                }
            }
            AsmHistoricalEntityKind::Vertex => {
                for edge in topology
                    .edge_vertices
                    .iter()
                    .filter(|edge| edge.start_vertex == entity_ref || edge.end_vertex == entity_ref)
                {
                    for face in edge_faces(edge.edge)? {
                        ctx.insert_hash_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )?;
                    }
                }
            }
            AsmHistoricalEntityKind::Point => {
                let mut vertices = HashSet::new();
                for binding in topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    ctx.insert_hash_set(
                        &mut vertices,
                        binding.entity,
                        "f3d historical point vertex candidate",
                    )?;
                }
                for edge in topology.edge_vertices.iter().filter(|edge| {
                    vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                }) {
                    for face in edge_faces(edge.edge)? {
                        ctx.insert_hash_set(
                            &mut faces,
                            face,
                            "f3d historical profile face candidate",
                        )?;
                    }
                }
            }
            AsmHistoricalEntityKind::Surface => {
                for binding in topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    ctx.insert_hash_set(
                        &mut faces,
                        binding.entity,
                        "f3d historical profile face candidate",
                    )?;
                }
            }
            AsmHistoricalEntityKind::Body
            | AsmHistoricalEntityKind::Region
            | AsmHistoricalEntityKind::Shell => {}
        }
    }
    Ok(faces)
}

#[derive(Debug, Clone, PartialEq)]
enum ResolvedProfileSelection {
    Loops(Vec<u32>),
    Regions(Vec<cadmpeg_ir::features::SketchProfileRegion>),
}

fn merge_resolved_profile_selections(
    sketch: &cadmpeg_ir::sketches::SketchId,
    selections: &[cadmpeg_ir::features::ProfileRef],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let mut profiles = Vec::new();
    let mut regions = Vec::new();
    for selection in selections {
        match selection {
            ProfileRef::Planar(PlanarProfileRef::SketchProfiles {
                sketch: selected,
                profiles: selected_profiles,
            }) if selected == sketch && regions.is_empty() => {
                for profile in selected_profiles.as_slice().iter().copied() {
                    if !profiles.contains(&profile) {
                        ctx.push_vec(&mut profiles, profile, "f3d merged selected profile")?;
                    }
                }
            }
            ProfileRef::Planar(PlanarProfileRef::SketchRegions {
                sketch: selected,
                regions: selected_regions,
            }) if selected == sketch && profiles.is_empty() => {
                for region in selected_regions.as_slice() {
                    if !regions.contains(region) {
                        let copied = copy_profile_region(region, ctx)?;
                        ctx.push_vec(&mut regions, copied, "f3d merged selected region")?;
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    let selected = if !profiles.is_empty() {
        PlanarProfileRef::sketch_profiles(
            (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?,
            profiles,
            ctx,
        )?
        .ok()
    } else if !regions.is_empty() {
        PlanarProfileRef::sketch_regions(
            (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?,
            regions,
        )
        .ok()
    } else {
        return Ok(None);
    };
    Ok(selected.map(ProfileRef::Planar))
}

fn copy_bound_profile(
    profile: &cadmpeg_ir::features::ProfileRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Planar(planar) => {
            Ok(ProfileRef::Planar(copy_bound_planar_profile(planar, ctx)?))
        }
        ProfileRef::SpatialSketchProfiles { sketch, profiles } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut indices = Vec::new();
            for index in profiles.as_slice() {
                ctx.push_vec(&mut indices, *index, "f3d bound spatial profile index")?;
            }
            ProfileRef::spatial_sketch_profiles(sketch, indices, ctx)?
                .map_err(CodecError::malformed)
        }
        ProfileRef::SpatialSketchSelection { sketch, selections } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut ids = Vec::new();
            for selection in selections.as_slice() {
                let id = ctx.copy_retained_text(selection, "f3d bound spatial selection id")?;
                ctx.push_vec(&mut ids, id, "f3d bound spatial selection")?;
            }
            ProfileRef::spatial_sketch_selection(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
    }
}

fn copy_bound_planar_profile(
    profile: &cadmpeg_ir::features::PlanarProfileRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PlanarProfileRef, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    match profile {
        PlanarProfileRef::Sketch(sketch) => Ok(PlanarProfileRef::Sketch(
            (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?,
        )),
        PlanarProfileRef::Native(id) => Ok(PlanarProfileRef::Native(
            ctx.copy_retained_text(id, "f3d bound native profile id")?,
        )),
        _ => Err(CodecError::malformed(
            "bound planar profile has unsupported resolved form",
        )),
    }
}

fn copy_bound_path(
    path: &cadmpeg_ir::features::PathRef,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::SketchCurves { sketch, curves } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile sketch id")?;
            let mut ids = Vec::new();
            for curve in curves.as_slice() {
                let id = (curve).try_clone_for_decode(ctx, "f3d bound planar curve id")?;
                ctx.push_vec(&mut ids, id, "f3d bound planar path curve")?;
            }
            PathRef::sketch_curves(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            let mut ids = Vec::new();
            for curve in curves.as_slice() {
                let id = (curve).try_clone_for_decode(ctx, "f3d bound spatial curve id")?;
                ctx.push_vec(&mut ids, id, "f3d bound spatial path curve")?;
            }
            PathRef::spatial_sketch_curves(sketch, ids, ctx)?.map_err(CodecError::malformed)
        }
        _ => Err(CodecError::malformed(
            "bound path has unsupported resolved form",
        )),
    }
}

fn copy_profile_boundary_use(
    boundary: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::SketchProfileBoundaryUse, CodecError> {
    let entity = boundary
        .entity
        .try_clone_for_decode(ctx, "f3d merged region boundary entity id")?;
    Ok(cadmpeg_ir::features::SketchProfileBoundaryUse {
        entity,
        parameter_range: boundary.parameter_range,
        reversed: boundary.reversed,
    })
}

fn copy_profile_region(
    region: &cadmpeg_ir::features::SketchProfileRegion,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::SketchProfileRegion, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    match region {
        SketchProfileRegion::Loops { loops } => {
            let mut holes = Vec::new();
            for hole in loops.holes().iter().copied() {
                ctx.push_vec(&mut holes, hole, "f3d merged region hole")?;
            }
            SketchProfileRegion::loops(loops.outer(), holes, ctx)?.map_err(CodecError::malformed)
        }
        SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } => {
            let mut outer = Vec::new();
            for boundary in outer_boundary.as_slice() {
                let copied = copy_profile_boundary_use(boundary, ctx)?;
                ctx.push_vec(&mut outer, copied, "f3d merged region outer boundary")?;
            }
            let mut holes = Vec::new();
            for ring in hole_boundaries {
                let mut copied_ring = Vec::new();
                for boundary in ring.as_slice() {
                    let copied = copy_profile_boundary_use(boundary, ctx)?;
                    ctx.push_vec(&mut copied_ring, copied, "f3d merged region hole boundary")?;
                }
                let copied_ring = copied_ring.try_into().map_err(CodecError::malformed)?;
                ctx.push_vec(&mut holes, copied_ring, "f3d merged region hole ring")?;
            }
            Ok(SketchProfileRegion::Trimmed {
                outer_boundary: outer.try_into().map_err(CodecError::malformed)?,
                hole_boundaries: holes,
            })
        }
    }
}

pub(super) fn resolved_extrude_profile_selection(
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let mut selection_members = Vec::new();
    for member in members.iter().filter(|member| {
        native_stream(&member.id) == native_stream(&group.id)
            && member.group_record_index == group.record_index
    }) {
        (resolution.ctx).push_vec(
            &mut selection_members,
            member,
            "f3d extrude selection member",
        )?;
    }
    resolution.ctx.stable_sort_by(
        &mut selection_members[..],
        |left, right| {
            let left_key = {
                let member = left;
                {
                    member.group_member_ordinal
                }
            };
            let right_key = {
                let member = right;
                {
                    member.group_member_ordinal
                }
            };
            left_key.cmp(&right_key)
        },
        |_| 0,
        "sort f3d design profile_select 3",
    )?;
    let exact_member_run = selection_members.len() == group.members().len()
        && selection_members
            .iter()
            .zip(group.members())
            .all(|(member, record_index)| member.record_index() == record_index.value);
    let mut resolved_profiles = None;
    if exact_member_run {
        let mut selected = Vec::new();
        let mut complete = true;
        for member in &selection_members {
            let Some(geometry) = member.resolved_geometry.as_ref() else {
                complete = false;
                break;
            };
            let SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            } = geometry
            else {
                complete = false;
                break;
            };
            let entity = crate::design::identity::neutral_sketch_curve_id(
                resolution.ctx,
                sketch_id,
                *primary_id,
                *secondary_id,
            )?;
            let mut matches = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity));
            let Some((index, _)) = matches.next() else {
                complete = false;
                break;
            };
            if matches.next().is_some() {
                complete = false;
                break;
            }
            let Ok(profile_index) = u32::try_from(index) else {
                complete = false;
                break;
            };
            if !selected.contains(&profile_index) {
                (resolution.ctx).push_vec(
                    &mut selected,
                    profile_index,
                    "f3d extrude selected profile",
                )?;
            }
        }
        if complete && !selected.is_empty() {
            resolved_profiles = Some(ResolvedProfileSelection::Loops(selected));
        }
    }
    if resolved_profiles.is_none() && exact_member_run {
        resolved_profiles = historical_selection_regions(
            &selection_members,
            sketch,
            resolution.entities,
            resolution.histories,
            resolution.linear_tolerance,
            resolution.arrangement_budget,
            resolution.ctx,
        )?;
    }
    if resolved_profiles.is_none() {
        if let (Some(state_id), Some(previous_state_id)) =
            (history_state_id, previous_history_state_id)
        {
            resolved_profiles =
                transition_profile_selection(sketch, resolution, state_id, previous_state_id)?;
        }
    }
    if resolved_profiles.is_none() && sketch.profiles.len() == 1 {
        resolved_profiles = Some(ResolvedProfileSelection::Loops(vec![0]));
    }
    let profile = match resolved_profiles {
        Some(ResolvedProfileSelection::Loops(profiles)) => match PlanarProfileRef::sketch_profiles(
            (sketch_id).try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?,
            profiles,
            resolution.ctx,
        )? {
            Ok(profile) => profile,
            Err(_) => PlanarProfileRef::Native(
                (resolution.ctx).copy_retained_text(&group.id, "f3d extrude fallback group id")?,
            ),
        },
        Some(ResolvedProfileSelection::Regions(regions)) => {
            match PlanarProfileRef::sketch_regions(
                (sketch_id).try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?,
                regions,
            ) {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(
                    (resolution.ctx)
                        .copy_retained_text(&group.id, "f3d extrude fallback group id")?,
                ),
            }
        }
        None => {
            let id =
                (resolution.ctx).copy_retained_text(&group.id, "f3d extrude selection group id")?;
            let mut ids = Vec::new();
            (resolution.ctx).push_vec(&mut ids, id, "f3d extrude selection group")?;
            match PlanarProfileRef::sketch_selection(
                (sketch_id).try_clone_for_decode(resolution.ctx, "f3d profile sketch id")?,
                ids,
                resolution.ctx,
            )? {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(
                    (resolution.ctx)
                        .copy_retained_text(&group.id, "f3d extrude fallback group id")?,
                ),
            }
        }
    };
    Ok(ProfileRef::Planar(profile))
}

fn transition_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    macro_rules! geometric {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let entities = resolution.entities;
    let arrangement_budget = resolution.arrangement_budget;
    let mut states = resolution
        .histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let state = geometric!(states.next());
    if states.next().is_some()
        || state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
    {
        return Ok(None);
    }
    let topology = geometric!(state.topology());
    let inserted_faces = &geometric!(state.transition.as_ref())
        .topology
        .faces
        .inserted;
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = resolution.linear_tolerance;
    let mut inserted_selections = Vec::new();
    for face in inserted_faces {
        let selection = match historical_face_points(*face, topology, resolution.ctx)? {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                &points,
                tolerance,
                arrangement_budget,
                resolution.ctx,
            )?,
            None => None,
        };
        (resolution.ctx).push_vec(
            &mut inserted_selections,
            selection,
            "f3d transition inserted selection",
        )?;
    }
    let inserted = transition_inserted_profile_selection(
        sketch,
        entities,
        tolerance,
        inserted_selections,
        resolution.ctx,
    )?;
    if inserted.is_some() {
        return Ok(inserted);
    }
    let mut cylindrical_selections = Vec::new();
    for face in inserted_faces {
        let selection = inserted_cylindrical_profile_selection(
            sketch,
            entities,
            topology,
            *face,
            tolerance,
            resolution.angular_tolerance,
            resolution.ctx,
        )?;
        (resolution.ctx).push_vec(
            &mut cylindrical_selections,
            selection,
            "f3d transition cylindrical selection",
        )?;
    }
    if let Some(selection) = unique_resolved_selection(cylindrical_selections) {
        return Ok(Some(selection));
    }
    let mut previous_states = resolution
        .histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let previous = geometric!(previous_states.next());
    if previous_states.next().is_some() {
        return Ok(None);
    }
    let previous_topology = geometric!(previous.topology());
    let deleted = &geometric!(state.transition.as_ref()).topology.faces.deleted;
    let faces = geometric!(unique_multi_face_deleted_carrier_family(
        deleted,
        previous_topology,
        resolution.ctx,
    )?);
    let mut selections = Vec::new();
    for face in faces {
        let selection = match historical_face_points(face, previous_topology, resolution.ctx)? {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                &points,
                tolerance,
                arrangement_budget,
                resolution.ctx,
            )?,
            None => None,
        };
        (resolution.ctx).push_vec(
            &mut selections,
            selection,
            "f3d transition deleted selection",
        )?;
    }
    ordered_unique_profile_selections(selections, resolution.ctx)
}

fn inserted_cylindrical_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
    linear_tolerance: f64,
    angular_tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut carriers = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let Some(carrier) = carriers.next().map(|binding| binding.carrier) else {
        return Ok(None);
    };
    if carriers.next().is_some() {
        return Ok(None);
    }
    let mut cylinders = topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| cylinder.surface == carrier);
    let Some(cylinder) = cylinders.next() else {
        return Ok(None);
    };
    if cylinders.next().is_some() || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
        return Ok(None);
    }
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return Ok(None);
    };
    let alignment = cylinder.axis.x * sketch_normal.x
        + cylinder.axis.y * sketch_normal.y
        + cylinder.axis.z * sketch_normal.z;
    if alignment.abs() < angular_tolerance.cos() {
        return Ok(None);
    }
    let offset = (sketch_origin.x - cylinder.origin.x) * sketch_normal.x
        + (sketch_origin.y - cylinder.origin.y) * sketch_normal.y
        + (sketch_origin.z - cylinder.origin.z) * sketch_normal.z;
    let parameter = offset / alignment;
    let Some(center) = project_to_sketch(
        sketch,
        Point3::new(
            cylinder.origin.x + parameter * cylinder.axis.x,
            cylinder.origin.y + parameter * cylinder.axis.y,
            cylinder.origin.z + parameter * cylinder.axis.z,
        ),
    ) else {
        return Ok(None);
    };
    let Some(points) = historical_face_points(face, topology, ctx)? else {
        return Ok(None);
    };
    let mut projected = Vec::new();
    for point in &points {
        let Some(point) = project_to_sketch(sketch, *point) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut projected,
            point,
            "f3d cylindrical profile projected point",
        )?;
    }
    let mut matches = sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            let [use_] = profile.as_slice() else {
                return None;
            };
            let entity = entities
                .iter()
                .find(|entity| entity.sketch == sketch.id && entity.id() == &use_.entity)?;
            let SketchGeometryDefinition::Circle {
                center: candidate_center,
                radius: candidate_radius,
            } = *entity.geometry.definition()
            else {
                return None;
            };
            ((candidate_center.u - center.u).hypot(candidate_center.v - center.v)
                <= linear_tolerance
                && (candidate_radius.get() - cylinder.radius).abs() <= linear_tolerance
                && projected.iter().all(|point| {
                    ((point.u - candidate_center.u).hypot(point.v - candidate_center.v)
                        - candidate_radius.get())
                    .abs()
                        <= linear_tolerance
                }))
            .then(|| u32::try_from(index).ok())?
        });
    let Some(profile) = matches.next() else {
        return Ok(None);
    };
    Ok(matches
        .next()
        .is_none()
        .then(|| ResolvedProfileSelection::Loops(vec![profile])))
}

fn resolved_spatial_extrude_profile_selection(
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<Option<u32>, CodecError> {
    enum ExactSelection {
        Resolved(u32),
        Unavailable,
        Contradictory,
    }

    let mut group_members = Vec::new();
    for member in members.iter().filter(|member| {
        native_stream(&member.id) == native_stream(&group.id)
            && member.group_record_index == group.record_index
    }) {
        (resolution.ctx).push_vec(
            &mut group_members,
            member,
            "f3d spatial profile group member",
        )?;
    }
    resolution.ctx.stable_sort_by(
        &mut group_members[..],
        |left, right| {
            let left_key = {
                let member = left;
                {
                    member.group_member_ordinal
                }
            };
            let right_key = {
                let member = right;
                {
                    member.group_member_ordinal
                }
            };
            left_key.cmp(&right_key)
        },
        |_| 0,
        "sort f3d design profile_select 4",
    )?;
    let exact_member_run = group_members.len() == group.members().len()
        && group_members
            .iter()
            .zip(group.members())
            .all(|(member, record_index)| member.record_index() == record_index.value);
    let exact_selection = (|| -> Result<ExactSelection, CodecError> {
        if !exact_member_run {
            return Ok(ExactSelection::Unavailable);
        }
        let mut selected = None;
        for member in group_members {
            let Some(SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            }) = member.resolved_geometry.as_ref()
            else {
                return Ok(ExactSelection::Unavailable);
            };
            let entity = crate::design::identity::neutral_spatial_sketch_curve_id(
                resolution.ctx,
                &sketch.id,
                *primary_id,
                *secondary_id,
            )?;
            let mut matches =
                sketch.profiles.iter().enumerate().filter(|(_, profile)| {
                    profile.boundary().iter().any(|use_| use_.entity == entity)
                });
            let Some((index, _)) = matches.next() else {
                return Ok(ExactSelection::Unavailable);
            };
            if matches.next().is_some() {
                return Ok(ExactSelection::Unavailable);
            }
            let Ok(profile) = u32::try_from(index) else {
                return Ok(ExactSelection::Unavailable);
            };
            if selected
                .replace(profile)
                .is_some_and(|selected| selected != profile)
            {
                return Ok(ExactSelection::Contradictory);
            }
        }
        Ok(selected.map_or(ExactSelection::Unavailable, ExactSelection::Resolved))
    })()?;
    match exact_selection {
        ExactSelection::Resolved(selection) => Ok(Some(selection)),
        ExactSelection::Contradictory => Ok(None),
        ExactSelection::Unavailable => {
            let transition = if let Some((state_id, previous_state_id)) =
                history_state_id.zip(previous_history_state_id)
            {
                transition_spatial_profile_selection(
                    sketch,
                    entities,
                    resolution.histories,
                    state_id,
                    previous_state_id,
                    resolution.linear_tolerance,
                    resolution.ctx,
                )?
            } else {
                None
            };
            Ok(transition.or_else(|| (sketch.profiles.len() == 1).then_some(0)))
        }
    }
}

fn transition_spatial_profile_selection(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let mut states = histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let Some(state) = states.next() else {
        return Ok(None);
    };
    if states.next().is_some()
        || state
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
            != Some(previous_state_id)
    {
        return Ok(None);
    }
    let Some(topology) = state.topology() else {
        return Ok(None);
    };
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let unique = |faces: &[i64],
                  topology: &crate::history_records::AsmHistoricalTopology|
     -> Result<Option<u32>, CodecError> {
        let mut indices = Vec::new();
        for face in faces {
            if let Some(points) = historical_face_points(*face, topology, ctx)? {
                if let Some(index) = spatial_polyline_profile_containing_points(
                    sketch, entities, &points, tolerance, ctx,
                )? {
                    ctx.push_vec(&mut indices, index, "f3d spatial transition profile index")?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut indices,
            Ord::cmp,
            |_| 0,
            "sort f3d spatial transition profile indices",
        )?;
        indices.dedup();
        Ok((indices.len() == 1).then(|| indices[0]))
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if let Some(index) = unique(&transition.topology.faces.inserted, topology)? {
        return Ok(Some(index));
    }
    let mut previous_states = histories
        .iter()
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == previous_state_id);
    let Some(previous) = previous_states.next() else {
        return Ok(None);
    };
    if previous_states.next().is_some() {
        return Ok(None);
    }
    let Some(previous_topology) = previous.topology() else {
        return Ok(None);
    };
    unique(&transition.topology.faces.deleted, previous_topology)
}

fn spatial_polyline_profile_containing_points(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    points: &[Point3],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let mut selected = None;
    for (index, profile) in sketch.profiles.iter().enumerate() {
        let mut offsets = points.iter().map(|point| {
            point
                .vector_from(profile.origin().get())
                .dot(profile.normal().into())
        });
        let Some(first) = offsets.next() else {
            continue;
        };
        if !offsets.all(|offset| (offset - first).abs() <= tolerance) {
            continue;
        }
        let normal = Vector3::from(profile.normal());
        let u_axis = Vector3::from(profile.u_axis());
        let v_axis = normal.cross(u_axis);
        let project = |point: Point3| {
            let offset = point.vector_from(profile.origin().get());
            Point2::new(offset.dot(u_axis), offset.dot(v_axis))
        };
        let mut polygon = Vec::new();
        for use_ in profile.boundary() {
            let Some(entity) = entities
                .iter()
                .find(|entity| entity.sketch == sketch.id && entity.id() == &use_.entity)
            else {
                return Ok(None);
            };
            let cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line { start, end } =
                entity.geometry.definition()
            else {
                return Ok(None);
            };
            let point = project(if use_.reversed {
                end.get()
            } else {
                start.get()
            });
            ctx.push_vec(&mut polygon, point, "f3d spatial profile polygon point")?;
        }
        if polygon.len() >= 3
            && points.iter().all(|point| {
                let point = project(*point);
                point_in_polygon(point, &polygon)
                    || polygon.iter().enumerate().any(|(index, start)| {
                        let end = polygon[(index + 1) % polygon.len()];
                        point_segment_distance(point, (*start, end)) <= tolerance
                    })
            })
        {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            if selected.replace(index).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(selected)
}

fn unique_multi_face_deleted_carrier_family(
    deleted_faces: &[i64],
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut seen = HashSet::new();
    let mut families = HashMap::<i64, Vec<i64>>::new();
    for face in deleted_faces.iter().copied() {
        if seen.contains(&face) {
            return Ok(None);
        }
        {
            ctx.reserve_set(&mut seen, 1, "f3d deleted face uniqueness index")?;
        }
        seen.insert(face);
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            return Ok(None);
        };
        if bindings.next().is_some() {
            return Ok(None);
        }
        ctx.push_hash_group(
            &mut families,
            carrier,
            face,
            "f3d deleted carrier family index",
            "f3d deleted carrier family face",
        )?;
    }
    let mut candidates = families.into_values().filter(|faces| faces.len() > 1);
    let Some(mut faces) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    ctx.sort_unstable_by(
        &mut faces,
        Ord::cmp,
        |_| 0,
        "sort f3d deleted carrier family faces",
    )?;
    Ok(Some(faces))
}

fn unique_resolved_selection<T: PartialEq>(
    selections: impl IntoIterator<Item = Option<T>>,
) -> Option<T> {
    let mut selections = selections.into_iter().flatten();
    let first = selections.next()?;
    selections
        .all(|selection| selection == first)
        .then_some(first)
}

fn transition_inserted_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    selections: Vec<Option<ResolvedProfileSelection>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let first_selection = selections.iter().flatten().next();
    if let Some(first) = first_selection {
        if selections
            .iter()
            .flatten()
            .all(|selection| selection == first)
        {
            return Ok(selections.into_iter().flatten().next());
        }
    }
    let mut loop_selections = selections
        .iter()
        .flatten()
        .filter_map(|selection| match selection {
            ResolvedProfileSelection::Loops(loops) if !loops.is_empty() => Some(loops.as_slice()),
            _ => None,
        });
    if let Some(first) = loop_selections.next() {
        if loop_selections.all(|candidate| candidate == first) {
            return Ok(selections.into_iter().flatten().find(|selection| {
                matches!(selection, ResolvedProfileSelection::Loops(loops) if !loops.is_empty())
            }));
        }
    }
    if selections.iter().flatten().all(|selection| {
        matches!(selection, ResolvedProfileSelection::Loops(loops) if !loops.is_empty())
    }) {
        let mut loops = Vec::new();
        for selection in selections.iter().flatten() {
            let ResolvedProfileSelection::Loops(selected) = selection else { continue; };
            for profile in selected.iter().copied() {
                if !loops.contains(&profile) {
                    ctx.push_vec(&mut loops, profile, "f3d inserted transition profile loop")?;
                }
            }
        }
        if !loops.is_empty()
            && profile_loops_are_independent(sketch, entities, &loops, tolerance, ctx)?
        {
            return Ok(Some(ResolvedProfileSelection::Loops(loops)));
        }
    }
    let mut regions = selections
        .iter()
        .flatten()
        .filter_map(|selection| match selection {
            ResolvedProfileSelection::Regions(regions) => match regions.as_slice() {
                [SketchProfileRegion::Loops { loops }] if !loops.holes().is_empty() => {
                    Some((loops.outer(), loops.holes()))
                }
                _ => None,
            },
            ResolvedProfileSelection::Loops(_) => None,
        });
    let Some((outer, holes)) = regions.next() else {
        return Ok(None);
    };
    if regions.any(|candidate| candidate != (outer, holes)) {
        return Ok(None);
    }
    let mut has_boundary_support = false;
    for selection in selections.iter().flatten() {
        match selection {
            ResolvedProfileSelection::Regions(regions)
                if matches!(
                    regions.as_slice(),
                    [SketchProfileRegion::Loops { loops }] if loops.outer() == outer && loops.holes() == holes
                ) => {}
            ResolvedProfileSelection::Loops(loops)
                if !loops.is_empty()
                    && loops
                        .iter()
                        .all(|profile| *profile == outer || holes.contains(profile)) =>
            {
                has_boundary_support = true;
            }
            _ => return Ok(None),
        }
    }
    if !has_boundary_support {
        return Ok(None);
    }
    let mut owned_holes = Vec::new();
    for hole in holes.iter().copied() {
        ctx.push_vec(
            &mut owned_holes,
            hole,
            "f3d inserted transition region hole",
        )?;
    }
    let Some(region) = SketchProfileRegion::loops(outer, owned_holes, ctx)?.ok() else {
        return Ok(None);
    };
    let mut output = Vec::new();
    ctx.push_vec(&mut output, region, "f3d inserted transition region")?;
    Ok(Some(ResolvedProfileSelection::Regions(output)))
}

pub(super) fn historical_face_points(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    let Some(loops) = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)
    else {
        return Ok(None);
    };
    let mut positions = Vec::new();
    for loop_ref in &loops.member_refs {
        let Some(coedges) = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)
        else {
            return Ok(None);
        };
        for coedge_ref in &coedges.member_refs {
            let Some(coedge) = topology
                .coedge_topology
                .iter()
                .find(|coedge| coedge.coedge == *coedge_ref)
            else {
                return Ok(None);
            };
            let Some(edge) = topology
                .edge_vertices
                .iter()
                .find(|edge| edge.edge == coedge.edge)
            else {
                return Ok(None);
            };
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                let Some(point_ref) = topology
                    .vertex_points
                    .iter()
                    .find(|binding| binding.entity == vertex_ref)
                    .map(|binding| binding.carrier)
                else {
                    return Ok(None);
                };
                let Some(position) = topology
                    .point_positions
                    .iter()
                    .find(|point| point.point == point_ref)
                    .map(|point| point.position)
                else {
                    return Ok(None);
                };
                if !positions.contains(&position) {
                    {
                        ctx.reserve_vec(&mut positions, 1, "f3d historical face point")?;
                    }
                    positions.push(position);
                }
            }
        }
    }
    Ok((positions.len() >= 3).then_some(positions))
}

fn historical_selection_regions(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    linear_tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let mut states = HashMap::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        if let Some(previous) = states.get_mut(&state.state_id) {
            *previous = None;
        } else {
            {
                ctx.reserve_map(&mut states, 1, "f3d historical selection state index")?;
            }
            states.insert(state.state_id, Some(state));
        }
    }
    let Some(first_member) = members.first() else {
        return Ok(None);
    };
    let mut state_ids = Vec::new();
    if let Some(binding) = first_member.historical.as_ref() {
        for state_id in binding.state_ids.iter().copied() {
            if !state_ids.contains(&state_id) {
                ctx.push_vec(
                    &mut state_ids,
                    state_id,
                    "f3d historical selection state id",
                )?;
            }
        }
    }
    for member in &members[1..] {
        state_ids.retain(|state_id| {
            member
                .historical
                .as_ref()
                .is_some_and(|binding| binding.state_ids.contains(state_id))
        });
    }
    ctx.sort_unstable_by(
        &mut state_ids,
        Ord::cmp,
        |_| 0,
        "sort f3d historical selection states",
    )?;
    let mut previous_member_points: Option<Vec<Vec<Point3>>> = None;
    let mut state_selection = None;
    let mut conflicting_state_selections = false;
    for state_id in state_ids {
        let Some(topology) = states
            .get(&state_id)
            .and_then(|state| state.as_ref())
            .and_then(|state| state.topology())
        else {
            continue;
        };
        let mut member_points = Vec::new();
        let mut complete = true;
        for member in members {
            let points = match historical_member_points_in_state(member, topology, ctx)? {
                Some(points) => Some(points),
                None => {
                    if let Some(point) =
                        resolved_selection_member_point(ctx, member, sketch, entities)?
                    {
                        let mut points = Vec::new();
                        ctx.push_vec(&mut points, point, "f3d historical fallback member point")?;
                        Some(points)
                    } else {
                        None
                    }
                }
            };
            let Some(points) = points else {
                complete = false;
                break;
            };
            ctx.push_vec(
                &mut member_points,
                points,
                "f3d historical selection member points",
            )?;
        }
        if !complete {
            continue;
        }
        if previous_member_points
            .as_ref()
            .is_some_and(|previous| same_member_point_bits(previous, &member_points))
        {
            continue;
        }
        let selection = selection_for_member_points(
            members,
            sketch,
            entities,
            &member_points,
            tolerance,
            arrangement_budget,
            ctx,
        )?;
        previous_member_points = Some(member_points);
        if let Some(selection) = selection {
            if let Some(previous) = state_selection.as_ref() {
                conflicting_state_selections |= previous != &selection;
            } else {
                state_selection = Some(selection);
            }
        }
    }
    if state_selection.is_some() {
        return Ok(if conflicting_state_selections {
            None
        } else {
            state_selection
        });
    }
    let mut member_points = Vec::new();
    let mut complete = true;
    for member in members {
        let Some(point) = resolved_selection_member_point(ctx, member, sketch, entities)? else {
            complete = false;
            break;
        };
        let mut points = Vec::new();
        ctx.push_vec(&mut points, point, "f3d resolved fallback member point")?;
        ctx.push_vec(
            &mut member_points,
            points,
            "f3d resolved fallback member points",
        )?;
    }
    if complete {
        if let Some(selection) = selection_for_member_points(
            members,
            sketch,
            entities,
            &member_points,
            tolerance,
            arrangement_budget,
            ctx,
        )? {
            return Ok(Some(selection));
        }
    }
    let mut selections = Vec::new();
    for member in members {
        let selection =
            if let Some(point) = resolved_selection_member_point(ctx, member, sketch, entities)? {
                selection_containing_points(
                    sketch,
                    entities,
                    std::slice::from_ref(&point),
                    tolerance,
                    arrangement_budget,
                    ctx,
                )?
            } else {
                resolved_selection_member_profiles(member, sketch, ctx)?
                    .map(ResolvedProfileSelection::Loops)
            };
        ctx.push_vec(
            &mut selections,
            selection,
            "f3d historical fallback selection",
        )?;
    }
    if ordered_selection_has_value(&selections) {
        return ordered_unique_profile_selections(selections, ctx);
    }
    region_with_boundary_selection_members(members, sketch, &selections, ctx)
}

fn same_member_point_bits(left: &[Vec<Point3>], right: &[Vec<Point3>]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| {
                    left.x.to_bits() == right.x.to_bits()
                        && left.y.to_bits() == right.y.to_bits()
                        && left.z.to_bits() == right.z.to_bits()
                })
        })
}

fn selection_for_member_points(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    member_points: &[Vec<Point3>],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut all_points = Vec::new();
    for point in member_points.iter().flatten().copied() {
        ctx.push_vec(
            &mut all_points,
            point,
            "f3d historical combined member point",
        )?;
    }
    if let Some(selection) = selection_containing_points(
        sketch,
        entities,
        &all_points,
        tolerance,
        arrangement_budget,
        ctx,
    )? {
        return Ok(Some(selection));
    }
    let mut selections = Vec::new();
    for points in member_points {
        let selection = selection_containing_points(
            sketch,
            entities,
            points,
            tolerance,
            arrangement_budget,
            ctx,
        )?;
        ctx.push_vec(
            &mut selections,
            selection,
            "f3d historical member selection",
        )?;
    }
    if ordered_selection_has_value(&selections) {
        return ordered_unique_profile_selections(selections, ctx);
    }
    region_with_boundary_selection_members(members, sketch, &selections, ctx)
}

fn region_with_boundary_selection_members(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    selections: &[Option<ResolvedProfileSelection>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let mut regions = selections.iter().filter_map(|selection| match selection {
        Some(ResolvedProfileSelection::Regions(regions)) => Some(regions.as_slice()),
        _ => None,
    });
    let Some([region]) = regions.next() else {
        return Ok(None);
    };
    let SketchProfileRegion::Loops { loops } = region else {
        return Ok(None);
    };
    if regions.any(|candidate| candidate != std::slice::from_ref(region)) {
        return Ok(None);
    }
    for (member, selection) in members.iter().zip(selections) {
        let matches = match selection {
            Some(ResolvedProfileSelection::Regions(candidate)) => {
                candidate == std::slice::from_ref(region)
            }
            Some(ResolvedProfileSelection::Loops(selected)) => {
                !selected.is_empty()
                    && selected
                        .iter()
                        .all(|profile| *profile == loops.outer() || loops.holes().contains(profile))
            }
            None => {
                resolved_selection_member_profiles(member, sketch, ctx)?.is_some_and(|profiles| {
                    !profiles.is_empty()
                        && profiles.iter().all(|profile| {
                            *profile == loops.outer() || loops.holes().contains(profile)
                        })
                })
            }
        };
        if !matches {
            return Ok(None);
        }
    }
    let mut owned_holes = Vec::new();
    for hole in loops.holes().iter().copied() {
        ctx.push_vec(
            &mut owned_holes,
            hole,
            "f3d historical boundary region hole",
        )?;
    }
    let Some(region) = SketchProfileRegion::loops(loops.outer(), owned_holes, ctx)?.ok() else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    ctx.push_vec(&mut selected, region, "f3d historical boundary region")?;
    Ok(Some(ResolvedProfileSelection::Regions(selected)))
}

fn resolved_selection_member_profiles(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(geometry) = member.resolved_geometry.as_ref() else {
        return Ok(None);
    };
    let SketchRelationOperand::Curve {
        primary_id,
        secondary_id,
        ..
    } = geometry
    else {
        return Ok(None);
    };
    let entity = crate::design::identity::neutral_sketch_curve_id(
        ctx,
        &sketch.id,
        *primary_id,
        *secondary_id,
    )?;
    let mut profiles = Vec::new();
    for (index, profile) in sketch.profiles.iter().enumerate() {
        if profile.iter().any(|use_| use_.entity == entity) {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            ctx.push_vec(&mut profiles, index, "f3d resolved member profile")?;
        }
    }
    Ok(Some(profiles))
}

fn resolved_selection_member_point(
    ctx: &DecodeContext<'_>,
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<Point3>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchRelationOperand::Point {
        record_index,
        persistent_id,
    } = or_none!(member.resolved_geometry.as_ref())
    else {
        return Ok(None);
    };
    let entity_id = persistent_id.map_or_else(
        || crate::design::identity::neutral_sketch_record_id(ctx, &sketch.id, *record_index),
        |persistent_id| {
            crate::design::identity::neutral_sketch_point_id(ctx, &sketch.id, persistent_id)
        },
    )?;
    let SketchGeometryDefinition::Point { position } = or_none!(entities
        .iter()
        .find(|entity| entity.id() == &entity_id && entity.sketch == sketch.id))
    .geometry
    .definition() else {
        return Ok(None);
    };
    let (origin, normal, u_axis) = or_none!(sketch.resolved_placement());
    let v_axis = normal.cross(u_axis.get());
    Ok(Some(
        origin
            .translated(u_axis.get(), position.u)
            .translated(v_axis, position.v),
    ))
}

fn ordered_selection_has_value(selections: &[Option<ResolvedProfileSelection>]) -> bool {
    let mut has_loops = false;
    let mut has_regions = false;
    for selection in selections {
        match selection {
            Some(ResolvedProfileSelection::Loops(selected)) if !has_regions => {
                has_loops |= !selected.is_empty();
            }
            Some(ResolvedProfileSelection::Regions(selected)) if !has_loops => {
                has_regions |= !selected.is_empty();
            }
            _ => return false,
        }
    }
    has_loops || has_regions
}

fn ordered_unique_profile_selections(
    matches: impl IntoIterator<Item = Option<ResolvedProfileSelection>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut loops = Vec::new();
    let mut regions = Vec::new();
    for selection in matches {
        let Some(selection) = selection else {
            return Ok(None);
        };
        match selection {
            ResolvedProfileSelection::Loops(selected) if regions.is_empty() => {
                for loop_index in selected {
                    if !loops.contains(&loop_index) {
                        ctx.push_vec(&mut loops, loop_index, "f3d ordered selected profile")?;
                    }
                }
            }
            ResolvedProfileSelection::Regions(selected) if loops.is_empty() => {
                for region in selected {
                    if !regions.contains(&region) {
                        ctx.push_vec(&mut regions, region, "f3d ordered selected region")?;
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    if !loops.is_empty() {
        Ok(Some(ResolvedProfileSelection::Loops(loops)))
    } else if !regions.is_empty() {
        Ok(Some(ResolvedProfileSelection::Regions(regions)))
    } else {
        Ok(None)
    }
}

fn selection_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    arrangement_budget: &WorkBudget<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut projected = Vec::new();
    for point in points {
        let Some(point) = project_to_sketch(sketch, *point) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut projected,
            point,
            "f3d historical projected selection point",
        )?;
    }
    let mut boundaries = Vec::new();
    for (index, profile) in sketch.profiles.iter().enumerate() {
        let mut matches = true;
        for point in &projected {
            let mut on_boundary = false;
            for use_ in profile {
                let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity)
                else {
                    continue;
                };
                if point_on_sketch_entity(ctx, *point, entity, tolerance)? {
                    on_boundary = true;
                    break;
                }
            }
            if !on_boundary {
                matches = false;
                break;
            }
        }
        if matches {
            let Some(index) = u32::try_from(index).ok() else {
                return Ok(None);
            };
            ctx.push_vec(&mut boundaries, index, "f3d historical boundary profile")?;
        }
    }
    if let [profile] = boundaries.as_slice() {
        let mut loops = Vec::new();
        ctx.push_vec(&mut loops, *profile, "f3d historical selected profile")?;
        return Ok(Some(ResolvedProfileSelection::Loops(loops)));
    }
    if let Some(region) = arrangement_region_containing_points(
        sketch,
        entities,
        &projected,
        tolerance,
        arrangement_budget,
        ctx,
    )? {
        let mut regions = Vec::new();
        ctx.push_vec(
            &mut regions,
            region,
            "f3d historical selected arrangement region",
        )?;
        return Ok(Some(ResolvedProfileSelection::Regions(regions)));
    }
    if !boundaries.is_empty() {
        return Ok(None);
    }
    let Some(region) = region_containing_points(sketch, entities, points, tolerance, ctx)? else {
        return Ok(None);
    };
    let mut regions = Vec::new();
    ctx.push_vec(
        &mut regions,
        region,
        "f3d historical selected geometric region",
    )?;
    Ok(Some(ResolvedProfileSelection::Regions(regions)))
}

/// Solved sketch records used to bind Loft and Revolve profile operands and
/// Loft guide selections.
pub(crate) struct SketchProfileResolution<'a> {
    pub(crate) entities: &'a [DesignEntityHeader],
    pub(crate) entity_selection_operands: &'a [DesignEntitySelectionOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
}

impl<'a> SketchProfileResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.entity_selection_operands,
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

/// Resolve one complete ordered entity-selection group to a neutral Sketch
/// path. The source identity pair is only meaningful with its owning Sketch
/// identity. Require one-to-one record ownership, a shared selection
/// namespace, a unique placement, and a unique neutral curve for every member
/// before exposing the path as typed geometry.
fn resolve_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PathRef>, CodecError> {
    use cadmpeg_ir::features::PathRef;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    macro_rules! available {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    if group.members().is_empty() {
        return Ok(None);
    }
    let stream = available!(native_stream(&group.id));
    let mut selected_identities = Vec::new();
    let mut member_records = HashSet::new();
    let mut primary_identity = None;
    let mut asset_id = None;
    let mut context_id = None;
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let ordinal = available!(u32::try_from(ordinal).ok());
        if !ctx.insert_hash_set(
            &mut member_records,
            record_index,
            "f3d entity path member record",
        )? {
            return Ok(None);
        }
        let mut matches = resolution.operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
        });
        let operand = available!(matches.next());
        if matches.next().is_some() {
            return Ok(None);
        }
        let secondary = available!(operand.secondary());
        if let Some(expected) = primary_identity {
            if expected != operand.primary_identity {
                return Ok(None);
            }
        } else {
            primary_identity = Some(operand.primary_identity);
        }
        if let Some(expected) = asset_id {
            if expected != operand.asset_id.as_str() {
                return Ok(None);
            }
        } else {
            asset_id = Some(operand.asset_id.as_str());
        }
        if let Some(expected) = context_id {
            if expected != operand.context_id.as_str() {
                return Ok(None);
            }
        } else {
            context_id = Some(operand.context_id.as_str());
        }
        ctx.push_vec(
            &mut selected_identities,
            secondary,
            "f3d entity path selected identity",
        )?;
    }
    let primary_identity = available!(primary_identity);
    let mut matching_placements = resolution.placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == primary_identity
    });
    let placement = available!(matching_placements.next());
    if matching_placements.next().is_some() {
        return Ok(None);
    }

    let mut curve_ids = Vec::new();
    let mut selected_curve_identities = HashSet::new();
    let owner_reference = available!(u32::try_from(primary_identity).ok());
    for secondary in &selected_identities {
        let secondary_identity = secondary.identity.value;
        let mut curves = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && curve.primary_id.get() == secondary_identity
                && secondary
                    .curve_identity
                    .is_none_or(|identity| curve.secondary_id == identity.value)
        });
        let curve = available!(curves.next());
        if curves.next().is_some() {
            return Ok(None);
        }
        let identity = (curve.primary_id.get(), curve.secondary_id);
        if !ctx.insert_hash_set(
            &mut selected_curve_identities,
            identity,
            "f3d entity path curve identity",
        )? {
            return Ok(None);
        }
        ctx.push_vec(&mut curve_ids, identity, "f3d entity path curve pair")?;
    }

    let spatial_sketch = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
    if resolution
        .spatial_sketches
        .iter()
        .any(|sketch| sketch.id == spatial_sketch)
    {
        let mut selections = HashSet::new();
        for (primary, secondary) in &curve_ids {
            let id = crate::design::identity::neutral_spatial_sketch_curve_id(
                ctx,
                &spatial_sketch,
                *primary,
                *secondary,
            )?;
            ctx.insert_hash_set(&mut selections, id, "f3d entity path spatial curve index")?;
        }
        if selections.len() != curve_ids.len()
            || selections.iter().any(|curve| {
                !resolution
                    .spatial_sketch_entities
                    .iter()
                    .any(|entity| entity.sketch == spatial_sketch && entity.id() == curve)
            })
        {
            return Ok(None);
        }
        let mut curves = Vec::new();
        for (primary, secondary) in curve_ids {
            let id = crate::design::identity::neutral_spatial_sketch_curve_id(
                ctx,
                &spatial_sketch,
                primary,
                secondary,
            )?;
            ctx.push_vec(&mut curves, id, "f3d entity path spatial output curve")?;
        }
        let sketch = spatial_sketch.try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
        return Ok(PathRef::spatial_sketch_curves(sketch, curves, ctx)?.ok());
    }

    let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
    if !resolution
        .sketches
        .iter()
        .any(|candidate| candidate.id == sketch)
    {
        return Ok(None);
    }
    let mut curves = Vec::new();
    for (primary, secondary) in curve_ids {
        let id =
            crate::design::identity::neutral_sketch_curve_id(ctx, &sketch, primary, secondary)?;
        ctx.push_vec(&mut curves, id, "f3d entity path planar output curve")?;
    }
    if curves.iter().any(|curve| {
        !resolution.sketch_entities.iter().any(|entity| {
            entity.sketch == sketch
                && entity.id() == curve
                && !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        })
    }) {
        return Ok(None);
    }
    Ok(PathRef::sketch_curves(sketch, curves, ctx)?.ok())
}

/// Resolve one ordered Loft guide or centerline group whose members select
/// curves from a Sketch.
fn resolved_loft_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &SketchProfileResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::PathRef>, CodecError> {
    if !matches!(
        group.role(),
        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
    ) {
        return Ok(None);
    }
    let path_resolution = resolution.path_resolution();
    resolve_entity_selection_path(group, &path_resolution, ctx)
}

fn spatial_profile_member_entity<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    curve_identities: &[SketchCurveIdentity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Result<Option<&'a cadmpeg_ir::sketches::SpatialSketchEntity>, CodecError> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
    });
    let curve = or_none!(curves.next());
    if curves.next().is_some() {
        return Ok(None);
    }
    let entity_id = crate::design::identity::neutral_spatial_sketch_curve_id(
        ctx,
        &spatial_sketch.id,
        curve.primary_id.get(),
        curve.secondary_id,
    )?;
    let mut entities = spatial_entities
        .iter()
        .filter(|entity| entity.sketch == spatial_sketch.id && entity.id() == &entity_id);
    let entity = or_none!(entities.next());
    Ok(entities.next().is_none().then_some(entity))
}

fn sketch_profile_member_entity<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<&'a cadmpeg_ir::sketches::SketchEntityId>, CodecError> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
    });
    let curve = or_none!(curves.next());
    if curves.next().is_some() {
        return Ok(None);
    }
    let entity_id = crate::design::identity::neutral_sketch_curve_id(
        ctx,
        &sketch.id,
        curve.primary_id.get(),
        curve.secondary_id,
    )?;
    let mut entities = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && entity.id() == &entity_id);
    let entity = or_none!(entities.next());
    if entities.next().is_some() {
        return Ok(None);
    }
    Ok(Some(entity.id()))
}

fn resolved_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(selection) = profile.region_selection.as_ref() else {
        return Ok(None);
    };
    let Ok(owner_reference) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut resolved = Vec::new();
    for region in &selection.regions {
        let Some(first_member) = region.members.first() else {
            return Ok(None);
        };
        let Some(first) = sketch_profile_member_entity(
            ctx,
            stream,
            owner_reference,
            first_member,
            sketch,
            curve_identities,
            sketch_entities,
        )?
        else {
            return Ok(None);
        };
        let mut matching_profiles = sketch
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| profile.iter().any(|use_| &use_.entity == first));
        let Some((profile_index, selected_profile)) = matching_profiles.next() else {
            return Ok(None);
        };
        if matching_profiles.next().is_some() {
            return Ok(None);
        }
        for member in &region.members[1..] {
            let Some(entity) = sketch_profile_member_entity(
                ctx,
                stream,
                owner_reference,
                member,
                sketch,
                curve_identities,
                sketch_entities,
            )?
            else {
                return Ok(None);
            };
            if !selected_profile.iter().any(|use_| &use_.entity == entity) {
                return Ok(None);
            }
        }
        let Ok(profile_index) = u32::try_from(profile_index) else {
            return Ok(None);
        };
        if resolved.contains(&profile_index) {
            return Ok(None);
        }
        ctx.push_vec(
            &mut resolved,
            profile_index,
            "f3d selected planar sketch profile region",
        )?;
    }
    Ok((!resolved.is_empty()).then_some(resolved))
}

fn coincident_spatial_profile_geometry(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Circle {
            center: first_center,
            normal: first_normal,
            radius: first_radius,
            ..
        },
        SpatialSketchGeometryDefinition::Circle {
            center: second_center,
            normal: second_normal,
            radius: second_radius,
            ..
        },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    if !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
        || !angular_tolerance.is_finite()
        || angular_tolerance < 0.0
    {
        return false;
    }
    let center_delta = Vector3::new(
        first_center.x - second_center.x,
        first_center.y - second_center.y,
        first_center.z - second_center.z,
    );
    let first_normal = *first_normal.to_unit_length_charted().as_raw();
    let second_normal = *second_normal.to_unit_length_charted().as_raw();
    let normal_angle = first_normal
        .cross(second_normal)
        .norm()
        .atan2(first_normal.dot(second_normal).abs());
    center_delta.norm() <= linear_tolerance
        && (first_radius.get() - second_radius.get()).abs() <= linear_tolerance
        && normal_angle <= angular_tolerance
}

fn resolved_spatial_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    resolution: &SketchProfileResolution<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(selection) = profile.region_selection.as_ref() else {
        if spatial_sketch.profiles.is_empty() {
            return Ok(None);
        }
        let mut profiles = Vec::new();
        for index in 0..spatial_sketch.profiles.len() {
            let Ok(index) = u32::try_from(index) else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut profiles,
                index,
                "f3d all spatial sketch profile regions",
            )?;
        }
        return Ok(Some(profiles));
    };
    let Ok(owner_reference) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut resolved = Vec::new();
    for region in &selection.regions {
        let Some(first_member) = region.members.first() else {
            return Ok(None);
        };
        let Some(first) = spatial_profile_member_entity(
            ctx,
            stream,
            owner_reference,
            first_member,
            spatial_sketch,
            resolution.curve_identities,
            resolution.spatial_sketch_entities,
        )?
        else {
            return Ok(None);
        };
        let mut matching_profiles =
            spatial_sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate
                        .boundary()
                        .iter()
                        .any(|use_| &use_.entity == first.id())
                });
        let Some((profile_index, selected_profile)) = matching_profiles.next() else {
            return Ok(None);
        };
        if matching_profiles.next().is_some() {
            return Ok(None);
        }
        for member in &region.members[1..] {
            let Some(entity) = spatial_profile_member_entity(
                ctx,
                stream,
                owner_reference,
                member,
                spatial_sketch,
                resolution.curve_identities,
                resolution.spatial_sketch_entities,
            )?
            else {
                return Ok(None);
            };
            if selected_profile
                .boundary()
                .iter()
                .any(|use_| &use_.entity == entity.id())
            {
                continue;
            }
            let coincident = selected_profile.boundary().iter().any(|use_| {
                resolution
                    .spatial_sketch_entities
                    .iter()
                    .find(|candidate| {
                        candidate.sketch == spatial_sketch.id && candidate.id() == &use_.entity
                    })
                    .is_some_and(|candidate| {
                        coincident_spatial_profile_geometry(
                            &candidate.geometry,
                            &entity.geometry,
                            resolution.linear_tolerance,
                            resolution.angular_tolerance,
                        )
                    })
            });
            if !coincident {
                return Ok(None);
            }
        }
        let Ok(profile_index) = u32::try_from(profile_index) else {
            return Ok(None);
        };
        if resolved.contains(&profile_index) {
            return Ok(None);
        }
        ctx.push_vec(
            &mut resolved,
            profile_index,
            "f3d selected spatial sketch profile region",
        )?;
    }
    Ok((!resolved.is_empty()).then_some(resolved))
}

fn spatial_profile_containing_entity(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntityId,
) -> Option<u32> {
    let mut profiles = sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.boundary().iter().any(|use_| use_.entity == *entity));
    let (index, _) = profiles.next()?;
    if profiles.next().is_some() {
        return None;
    }
    u32::try_from(index).ok()
}

pub(crate) fn bind_loft_and_revolve_sketch_selections(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    resolution: &SketchProfileResolution<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftSection, PathRef, PlanarProfileRef, ProfileRef,
    };

    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        ctx.insert_hash_map(
            &mut header_index,
            (stream, header.record_index),
            header,
            "f3d loft header index",
        )
        .map(|_| ())?;
    }
    let mut resolved_profiles = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(
            group.role(),
            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
        ) && group.members().len() == 1
    }) {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(header) = header_index.get(&(stream, group.members()[0].value)) else {
            continue;
        };
        let Some(profile) = parse_sketch_profile(
            ctx,
            bytes,
            stream,
            group.scope_reference_ordinal,
            header,
            resolution.entities,
        )
        .transpose()?
        else {
            continue;
        };
        let mut matches = resolution.placements.iter().filter(|placement| {
            native_stream(&placement.id) == Some(stream) && placement.entity_id == profile.entity_id
        });
        let Some(placement) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let spatial_sketch_id = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
        let resolved = if let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        {
            let id =
                spatial_sketch_id.try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
            if let Some(profiles) = resolved_spatial_sketch_profile_regions(
                stream,
                &profile,
                spatial_sketch,
                resolution,
                ctx,
            )? {
                match ProfileRef::spatial_sketch_profiles(id, profiles, ctx)? {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                        ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
                    )),
                }
            } else {
                let group_id =
                    ctx.copy_retained_text(&group.id, "f3d loft spatial selection id")?;
                match ProfileRef::spatial_sketch_selection(id, vec![group_id], ctx)? {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                        ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
                    )),
                }
            }
        } else {
            let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
            if !resolution
                .sketches
                .iter()
                .any(|candidate| candidate.id == sketch)
            {
                continue;
            }
            ProfileRef::Planar(PlanarProfileRef::Sketch(sketch))
        };
        ctx.insert_hash_map(
            &mut resolved_profiles,
            group.id.as_str(),
            resolved,
            "f3d loft resolved profile index",
        )
        .map(|_| ())?;
    }
    let mut resolved_entity_paths = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(
            group.role(),
            DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
        ) && !group.members().is_empty()
    }) {
        if let Some(path) = resolved_loft_entity_selection_path(group, resolution, ctx)? {
            ctx.insert_hash_map(
                &mut resolved_entity_paths,
                group.id.as_str(),
                path,
                "f3d loft resolved path index",
            )
            .map(|_| ())?;
        }
    }
    for group in groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5 && group.members().len() == 1)
    {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut operands = resolution
            .entity_selection_operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index() == group.members()[0].value
            });
        let Some(operand) = operands.next() else {
            continue;
        };
        if operands.next().is_some() {
            continue;
        }
        let mut matching_placements = resolution.placements.iter().filter(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_id.suffix() == operand.primary_identity
        });
        let Some(placement) = matching_placements.next() else {
            continue;
        };
        if matching_placements.next().is_some() {
            continue;
        }
        let spatial_sketch_id = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
        let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        else {
            continue;
        };
        let Ok(owner_reference) = u32::try_from(operand.primary_identity) else {
            continue;
        };
        let mut geometry_matches = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && entity_selection_matches_curve(operand, curve)
        });
        let Some(curve) = geometry_matches.next() else {
            continue;
        };
        if geometry_matches.next().is_some() {
            continue;
        }
        let entity = crate::design::identity::neutral_spatial_sketch_curve_id(
            ctx,
            &spatial_sketch_id,
            curve.primary_id.get(),
            curve.secondary_id,
        )?;
        let profile = spatial_profile_containing_entity(spatial_sketch, &entity);
        let id = spatial_sketch_id.try_clone_for_decode(ctx, "f3d profile spatial sketch id")?;
        let resolved = if let Some(profile) = profile {
            ProfileRef::spatial_sketch_profiles(id, vec![profile], ctx)?
        } else {
            let operand_id =
                ctx.copy_retained_text(&operand.id, "f3d loft entity selection operand id")?;
            ProfileRef::spatial_sketch_selection(id, vec![operand_id], ctx)?
        };
        let resolved = match resolved {
            Ok(profile) => profile,
            Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                ctx.copy_retained_text(&group.id, "f3d loft native profile id")?,
            )),
        };
        ctx.insert_hash_map(
            &mut resolved_profiles,
            group.id.as_str(),
            resolved,
            "f3d loft resolved profile index",
        )
        .map(|_| ())?;
    }
    for feature in features.iter_mut() {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Loft {
                    sections, guidance, ..
                }) = definition
                else {
                    return Ok(());
                };
                for section in sections.iter_mut() {
                    let LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(native))) =
                        section
                    else {
                        continue;
                    };
                    if let Some(profile) = resolved_profiles.get(native.as_str()) {
                        *section = LoftSection::Profile(copy_bound_profile(profile, ctx)?);
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                        for guide in guides.iter_mut() {
                            let PathRef::Native(native) = guide else {
                                continue;
                            };
                            if let Some(path) = resolved_entity_paths.get(native.as_str()) {
                                *guide = copy_bound_path(path, ctx)?;
                            }
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                        if let PathRef::Native(native) = centerline {
                            if let Some(path) = resolved_entity_paths.get(native.as_str()) {
                                *centerline = copy_bound_path(path, ctx)?;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    for feature in features.iter_mut() {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) = definition
                else {
                    return Ok(());
                };
                let Some(PlanarProfileRef::Native(native)) = construction.profile() else {
                    return Ok(());
                };
                let Some(ProfileRef::Planar(profile)) = resolved_profiles.get(native.as_str())
                else {
                    return Ok(());
                };
                construction.set_profile(Some(copy_bound_planar_profile(profile, ctx)?));
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;
