// SPDX-License-Identifier: Apache-2.0
//! Historical feature selection and persistent identity resolution.

use super::{
    bound_scope_history, design_axis, effective_scope_previous_history_state_id,
    historical_edge_axis, history_state_reaches, projection_was_finalized, same_axis_line,
    stable_ref, treatment_edge_candidates, unique_history_state_pair,
    EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9, EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9,
    EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8, EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9,
    HOLE_SUPPORT_NORMAL_TOLERANCE, HOLE_SUPPORT_POINT_TOLERANCE,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use crate::records::{
    bodies::DesignBodyBinding,
    recipes::DesignComponentNamingSpace,
    topology::{
        body_recipe::AsmHistoricalEntityKind, edge_identity::DesignEdgeIdentityOperand,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) fn boundary_edges_in_changes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    boundary_edges: &[i64],
    changes: &[i64],
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    decode.collect_vec(
        boundary_edges
            .iter()
            .copied()
            .filter(|edge| changes.contains(edge)),
        "collect F3D boundary edges in changes",
    )
}

pub(super) fn recipe_selector_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    contexts: &[crate::records::topology::historical_context::DesignHistoricalEdgeContext],
) -> Result<
    Vec<crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext>,
    cadmpeg_core::CodecError,
> {
    let Some(structure) = structure else {
        return Ok(Vec::new());
    };
    let mut selectors = BTreeSet::new();
    for selector in structure
        .sides
        .iter()
        .flat_map(|side| side.entries.iter().map(|entry| entry.selector))
    {
        decode
            .insert_btree_set(&mut selectors, selector, "index F3D edge recipe selectors")
            .map(|_| ())?;
    }
    let mut selected = Vec::new();
    for selector in selectors {
        let mut clauses = Vec::new();
        for side in &structure.sides {
            let clause = if let Some(entry) =
                side.entries.iter().find(|entry| entry.selector == selector)
            {
                let [first, second] = entry.topology_triplets.each_ref().map(|triplet| {
                    decode.collect_vec(
                        contexts
                            .iter()
                            .filter(|context| {
                                context.incident_loops.iter().any(|incident| {
                                    incident.boundary_edge_count == entry.boundary_edge_count.get()
                                        && triplet
                                            .incident
                                            .map(|incident| incident.ordinal)
                                            .is_some_and(|ordinal| {
                                                incident.coedge_ordinal == ordinal
                                            })
                                })
                            })
                            .map(|context| context.edge_slot),
                        "collect F3D recipe triplet edges",
                    )
                });
                Some(
                    crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
                        entry: *entry,
                        triplet_edge_slots: [first?, second?],
                    },
                )
            } else {
                None
            };

            decode.reserve_vec(&mut clauses, 1, "collect F3D recipe selector clauses")?;
            clauses.push(clause);
        }
        let required = decode.collect_vec(
            clauses.iter().map(|entry| {
                entry
                    .as_ref()
                    .map(|entry| i64::from(entry.entry.boundary_edge_count.get()))
            }),
            "collect F3D recipe side counts",
        )?;
        let mut boundary_count_matching_edge_slots = Vec::new();
        for context in contexts {
            let counts = decode.collect_vec(
                context
                    .incident_loops
                    .iter()
                    .map(|incident| i64::from(incident.boundary_edge_count)),
                "collect F3D incident loop counts",
            )?;
            if incident_loop_counts_satisfy_sides(&counts, &required) {
                decode.reserve_vec(
                    &mut boundary_count_matching_edge_slots,
                    1,
                    "collect F3D boundary count edge matches",
                )?;
                boundary_count_matching_edge_slots.push(context.edge_slot);
            }
        }
        let incidence_matching_edge_slots = decode.collect_vec(
            contexts
                .iter()
                .filter(|context| {
                    clauses.iter().flatten().all(|clause| {
                        let entry = &clause.entry;
                        entry.topology_triplets.iter().all(|triplet| {
                            context.incident_loops.iter().any(|incident| {
                                incident.boundary_edge_count == entry.boundary_edge_count.get()
                                    && triplet
                                        .incident
                                        .map(|incident| incident.ordinal)
                                        .is_some_and(|ordinal| incident.coedge_ordinal == ordinal)
                            })
                        })
                    })
                })
                .map(|context| context.edge_slot),
            "collect F3D incidence edge matches",
        )?;

        decode.reserve_vec(&mut selected, 1, "collect F3D edge recipe selectors")?;
        selected.push(
            crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
                selector,
                clauses,
                incidence_matching_edge_slots,
                boundary_count_matching_edge_slots,
            },
        );
    }
    Ok(selected)
}

pub(super) fn historical_edge_context(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<
    crate::records::topology::historical_context::DesignHistoricalEdgeContext,
    cadmpeg_core::CodecError,
> {
    let mut incident_loops = decode.collect_vec(
        topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.edge == edge)
            .filter_map(|coedge| {
                let loop_relation = topology
                    .loop_coedges
                    .iter()
                    .find(|relation| relation.owner_ref == coedge.owner_loop)?;
                let ordinal = loop_relation
                    .member_refs
                    .iter()
                    .position(|candidate| *candidate == coedge.coedge)?;
                let boundary_edge_count = u32::try_from(loop_relation.member_refs.len()).ok()?;
                let coedge_ordinal = u32::try_from(ordinal).ok()?;
                let previous_coedge = loop_relation.member_refs.get(
                    (ordinal + loop_relation.member_refs.len() - 1)
                        % loop_relation.member_refs.len(),
                )?;
                let next_coedge = loop_relation
                    .member_refs
                    .get((ordinal + 1) % loop_relation.member_refs.len())?;
                let edge_for_coedge = |slot| {
                    topology
                        .coedge_topology
                        .iter()
                        .find(|candidate| candidate.coedge == slot)
                        .map(|candidate| candidate.edge)
                };
                let face_slot = topology
                    .face_loops
                    .iter()
                    .find(|relation| relation.member_refs.contains(&coedge.owner_loop))?
                    .owner_ref;
                Some(
                    crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                        coedge_slot: coedge.coedge,
                        loop_slot: coedge.owner_loop,
                        face_slot,
                        boundary_edge_count,
                        coedge_ordinal,
                        previous_edge_slot: edge_for_coedge(*previous_coedge)?,
                        next_edge_slot: edge_for_coedge(*next_coedge)?,
                    },
                )
            }),
        "collect F3D historical incident loops",
    )?;
    decode.stable_sort_by(
        &mut incident_loops,
        |left, right| left.coedge_slot.cmp(&right.coedge_slot),
        |_| 0,
        "sort F3D historical incident loops",
    )?;
    Ok(
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: edge,
            incident_loops,
        },
    )
}

pub(super) fn incident_loop_counts_satisfy_sides(counts: &[i64], required: &[Option<i64>]) -> bool {
    required.iter().enumerate().all(|(ordinal, value)| {
        let Some(value) = value else {
            return true;
        };
        let available = counts.iter().filter(|count| *count == value).count();
        let needed = required[..=ordinal]
            .iter()
            .filter(|candidate| *candidate == &Some(*value))
            .count();
        available >= needed
    })
}

pub(super) fn bind_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::face::DesignFaceOperand],
    updated_face_slots: &[i64],
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::FaceSelection::Native(native) = selection else {
        return Ok(());
    };
    if native == &scope.id {
        if let Some(resolved) =
            crate::design::feature_project::direct_face_selection(ctx, scope, operands)?
        {
            if !matches!(resolved, cadmpeg_ir::features::FaceSelection::Native(_)) {
                *selection = resolved;
            }
        }
        return Ok(());
    }
    let mut matching_groups = groups.iter().filter(|group| group.id == *native);
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some()
        || group.scope_record_index != scope.record_index
        || crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
    {
        return Ok(());
    }
    if let Some(resolved) =
        crate::design::face_resolve::resolved_historical_split_face_target_group_with_updated_faces(
            ctx,
            scope,
            scope.previous_history_state_id(),
            group,
            operands,
            updated_face_slots,
        )?
    {
        *selection = resolved;
        return Ok(());
    }
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return Ok(());
    };
    let mut faces = Vec::new();
    for record_index in group.members().iter().map(|member| &member.value) {
        let mut matches = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.record_index() == *record_index
        });
        let Some(operand) = matches.next() else {
            return Ok(());
        };
        if matches.next().is_some() {
            return Ok(());
        }
        let previous_candidates = &operand.preceding_candidate_faces;
        let candidate = match previous_candidates.as_slice() {
            [face] => face,
            _ => {
                let [face] = operand.changed_candidate_faces.as_slice() else {
                    return Ok(());
                };
                face
            }
        };
        if faces.contains(candidate) {
            continue;
        }
        if !operand.candidate_faces.contains(candidate) {
            return Ok(());
        }

        ctx.reserve_vec(&mut faces, 1, "collect F3D resolved face selection")?;
        let face = candidate.try_clone_for_decode(ctx, "copy F3D resolved face identity")?;
        faces.push(face);
    }
    if !faces.is_empty() {
        *selection = cadmpeg_ir::features::FaceSelection::Resolved {
            faces,
            native: ctx.copy_retained_text(native, "copy F3D resolved face selection identity")?,
        };
    }
    Ok(())
}

pub(super) fn bind_body_recipe_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(native) = selection else {
        return Ok(());
    };
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *native
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X5
            && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == *record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(slot) = operand.resolved_face_slot else {
            return Ok(());
        };
        if !slots.contains(&slot) {
            ctx.reserve_vec(&mut slots, 1, "collect F3D body recipe face slots")?;
            slots.push(slot);
        }
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut faces = Vec::new();
    for slot in slots {
        let face =
            crate::ids::history_input_face_id_charged(ctx, feature_id, previous_state_id, slot)?;

        ctx.reserve_vec(&mut faces, 1, "collect F3D body recipe face identities")?;
        faces.push(face);
    }
    let native = ctx.copy_retained_text(native, "copy F3D body recipe face selection identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state, faces, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

pub(super) fn faces_in_topology<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: impl IntoIterator<Item = &'a cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut faces = HashSet::new();
    for face in &topology.faces {
        decode.insert_hash_set(&mut faces, *face, "index F3D topology faces")?;
    }
    decode.try_collect_vec(
        candidates
            .into_iter()
            .filter(|face| stable_ref(face.as_str()).is_some_and(|slot| faces.contains(&slot)))
            .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
        "collect F3D faces in topology",
    )
}

/// Topology families and states containing one requested stable ASM slot.
#[derive(Default)]
struct HistoricalIdentityMembership {
    kinds: HashSet<AsmHistoricalEntityKind>,
    states: Vec<i64>,
}

#[derive(Default)]
struct HistoricalRevisionMembership {
    entity_refs: HashSet<i64>,
    states: Vec<i64>,
}

/// Ambiguity-aware ASM identity data restricted to the Design identities a
/// binding pass requests.
pub(super) struct HistoricalIdentityIndex {
    identities: HashMap<i64, HistoricalIdentityMembership>,
    revisions: HashMap<i64, HistoricalRevisionMembership>,
}

impl HistoricalIdentityIndex {
    pub(super) fn build<'a>(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        histories: impl IntoIterator<Item = &'a AsmHistory>,
        local_ids: impl IntoIterator<Item = u64>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let histories = decode.collect_vec(histories, "collect F3D identity histories")?;
        let mut record_refs = HashSet::new();
        for record_ref in local_ids
            .into_iter()
            .filter_map(|local_id| i64::try_from(local_id).ok())
        {
            decode.insert_hash_set(
                &mut record_refs,
                record_ref,
                "index F3D identity record refs",
            )?;
        }
        let mut identities = HashMap::<i64, HistoricalIdentityMembership>::new();
        let mut revisions = HashMap::<i64, HistoricalRevisionMembership>::new();
        if record_refs.is_empty() {
            return Ok(Self {
                identities,
                revisions,
            });
        }
        if histories
            .iter()
            .any(|history| history.record_table_binding_budget_exceeded)
        {
            return Ok(Self {
                identities,
                revisions,
            });
        }
        let ambiguous_states = ambiguous_history_state_ids(decode, &histories)?;
        for state in histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| !ambiguous_states.contains(&state.state_id))
        {
            for version in &state.entity_versions {
                if record_refs.contains(&version.record_ref) {
                    if !revisions.contains_key(&version.record_ref) {
                        decode.reserve_map(&mut revisions, 1, "index F3D revision membership")?;
                    }
                    let membership = revisions.entry(version.record_ref).or_default();
                    decode.insert_hash_set(
                        &mut membership.entity_refs,
                        version.entity_ref,
                        "index F3D revision entity refs",
                    )?;
                    if !membership.states.contains(&state.state_id) {
                        decode.reserve_vec(
                            &mut membership.states,
                            1,
                            "collect F3D revision states",
                        )?;
                        membership.states.push(state.state_id);
                    }
                }
            }
        }
        let mut versioned_revisions = HashSet::new();
        for revision in revisions.keys() {
            decode.insert_hash_set(
                &mut versioned_revisions,
                *revision,
                "index F3D versioned revisions",
            )?;
        }
        let mut reconstructed_revisions = HashSet::new();
        // Finalized histories retain the entity-slot membership and the
        // bulletin-board chain after their geometry caches are compacted.
        for history in histories.iter().filter(|history| {
            !history.states.is_empty()
                && history
                    .states
                    .iter()
                    .all(|state| match &state.topology_cache {
                        crate::history_records::AsmTopologyCache::Absent => false,
                        crate::history_records::AsmTopologyCache::Complete(_) => true,
                        crate::history_records::AsmTopologyCache::Retained(_) => {
                            history.projection_finalized()
                        }
                        crate::history_records::AsmTopologyCache::Released => false,
                    })
        }) {
            for change in history
                .states
                .iter()
                .flat_map(|state| &state.bulletin_boards)
                .flat_map(|board| &board.changes)
            {
                let Some(record_ref) = change.old_ref().filter(|old| record_refs.contains(old))
                else {
                    continue;
                };
                let entity_ref = change.new_ref().unwrap_or(record_ref);
                if !revisions.contains_key(&record_ref) {
                    decode.reserve_map(&mut revisions, 1, "index F3D reconstructed revisions")?;
                }
                let revision = revisions.entry(record_ref).or_default();
                decode.insert_hash_set(
                    &mut revision.entity_refs,
                    entity_ref,
                    "index F3D reconstructed entity refs",
                )?;
                if !versioned_revisions.contains(&record_ref) {
                    decode.insert_hash_set(
                        &mut reconstructed_revisions,
                        record_ref,
                        "index F3D reconstructed revision refs",
                    )?;
                }
            }
        }
        let mut entity_refs = HashSet::new();
        for entity_ref in record_refs.iter().copied().chain(
            revisions
                .values()
                .flat_map(|revision| revision.entity_refs.iter().copied()),
        ) {
            decode.insert_hash_set(
                &mut entity_refs,
                entity_ref,
                "index F3D identity entity refs",
            )?;
        }
        for state in histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| !ambiguous_states.contains(&state.state_id))
        {
            let Some(topology) = state.topology() else {
                continue;
            };
            let families: [(AsmHistoricalEntityKind, &[i64]); 12] = [
                (AsmHistoricalEntityKind::Body, &topology.bodies),
                (AsmHistoricalEntityKind::Region, &topology.regions),
                (AsmHistoricalEntityKind::Shell, &topology.shells),
                (AsmHistoricalEntityKind::Face, &topology.faces),
                (AsmHistoricalEntityKind::Loop, &topology.loops),
                (AsmHistoricalEntityKind::Coedge, &topology.coedges),
                (AsmHistoricalEntityKind::Edge, &topology.edges),
                (AsmHistoricalEntityKind::Vertex, &topology.vertices),
                (AsmHistoricalEntityKind::Point, &topology.points),
                (AsmHistoricalEntityKind::Surface, &topology.surfaces),
                (AsmHistoricalEntityKind::Curve, &topology.curves),
                (AsmHistoricalEntityKind::Pcurve, &topology.pcurves),
            ];
            for (kind, members) in families {
                for entity_ref in members
                    .iter()
                    .filter(|entity_ref| entity_refs.contains(entity_ref))
                {
                    if !identities.contains_key(entity_ref) {
                        decode.reserve_map(&mut identities, 1, "index F3D identity membership")?;
                    }
                    let membership = identities.entry(*entity_ref).or_default();
                    decode.insert_hash_set(
                        &mut membership.kinds,
                        kind,
                        "index F3D identity kinds",
                    )?;
                    if !membership.states.contains(&state.state_id) {
                        decode.reserve_vec(
                            &mut membership.states,
                            1,
                            "collect F3D identity states",
                        )?;
                        membership.states.push(state.state_id);
                    }
                }
            }
        }
        for record_ref in reconstructed_revisions {
            let Some(revision) = revisions.get_mut(&record_ref) else {
                continue;
            };
            revision.states = decode.collect_vec(
                revision
                    .entity_refs
                    .iter()
                    .filter_map(|entity_ref| identities.get(entity_ref))
                    .flat_map(|membership| membership.states.iter().copied()),
                "collect F3D reconstructed revision states",
            )?;
            decode.sort_unstable_by(
                &mut revision.states,
                Ord::cmp,
                |_| 0,
                "sort F3D reconstructed revision states",
            )?;
            revision.states.dedup();
        }
        Ok(Self {
            identities,
            revisions,
        })
    }

    fn identity_kind(
        &self,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        local_id: u64,
    ) -> Result<Option<(AsmHistoricalEntityKind, Vec<i64>)>, cadmpeg_core::CodecError> {
        let Some(entity_ref) = i64::try_from(local_id).ok() else {
            return Ok(None);
        };
        let Some(membership) = self.identities.get(&entity_ref) else {
            return Ok(None);
        };
        let mut kinds = membership.kinds.iter();
        let Some(kind) = kinds.next().copied() else {
            return Ok(None);
        };
        if kinds.next().is_some() {
            return Ok(None);
        }
        let states = decode.collect_vec(
            membership.states.iter().copied(),
            "copy F3D identity states",
        )?;
        Ok(Some((kind, states)))
    }

    fn selection_identity_kind(
        &self,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        local_id: u64,
    ) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
        let Some(record_ref) = i64::try_from(local_id).ok() else {
            return Ok(None);
        };
        let revision = self.revisions.get(&record_ref);
        if let Some((kind, states)) = self.identity_kind(decode, local_id)? {
            return Ok(revision
                .is_none_or(|revision| {
                    revision.entity_refs.is_empty()
                        || (revision.entity_refs.len() == 1
                            && revision.entity_refs.contains(&record_ref))
                })
                .then_some((kind, record_ref, states)));
        }
        let Some(revision) = revision else {
            return Ok(None);
        };
        let mut entity_refs = revision.entity_refs.iter();
        let Some(entity_ref) = entity_refs.next().copied() else {
            return Ok(None);
        };
        if entity_refs.next().is_some() {
            return Ok(None);
        }
        let Some(entity_id) = u64::try_from(entity_ref).ok() else {
            return Ok(None);
        };
        let Some((kind, _)) = self.identity_kind(decode, entity_id)? else {
            return Ok(None);
        };
        let states =
            decode.collect_vec(revision.states.iter().copied(), "copy F3D revision states")?;
        Ok(Some((kind, entity_ref, states)))
    }
}

#[cfg(test)]
pub(super) fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    crate::test_support::with_decode_context(|decode_ctx| {
        HistoricalIdentityIndex::build(decode_ctx, histories, [local_id])
            .expect("test identity index allocation")
            .identity_kind(decode_ctx, local_id)
    })
    .expect("test identity kind allocation")
}

pub(super) fn historical_selection_identity_kind(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[AsmHistory],
    local_id: u64,
) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    HistoricalIdentityIndex::build(decode, histories, [local_id])?
        .selection_identity_kind(decode, local_id)
}

fn ambiguous_history_state_ids(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[&AsmHistory],
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let mut unique = HashSet::new();
    let mut ambiguous = HashSet::new();
    for state in histories.iter().flat_map(|history| &history.states) {
        if !decode.insert_hash_set(
            &mut unique,
            state.state_id,
            "index F3D unique history states",
        )? {
            decode.insert_hash_set(
                &mut ambiguous,
                state.state_id,
                "index F3D ambiguous history states",
            )?;
        }
    }
    Ok(ambiguous)
}

/// Select the complete BREP-history set owned by one component context.
/// `None` means the stream predates component naming-space registrations and
/// retains the aggregate compatibility path. `Some(empty)` is a known
/// component with no history-bearing BREP.
fn component_histories<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    context_id: &str,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &'a [AsmHistory],
) -> Result<Option<Vec<&'a AsmHistory>>, cadmpeg_core::CodecError> {
    if naming_spaces.is_empty() {
        return Ok(None);
    }
    let mut matching_spaces = naming_spaces
        .iter()
        .filter(|space| space.context_uuid.as_str().eq_ignore_ascii_case(context_id));
    let Some(space) = matching_spaces.next() else {
        return Ok(Some(Vec::new()));
    };
    if matching_spaces.next().is_some() {
        return Ok(Some(Vec::new()));
    }
    let Some(stream) = crate::ids::native_stream(&space.id) else {
        return Ok(Some(Vec::new()));
    };
    let cluster_end = naming_spaces
        .iter()
        .filter(|candidate| {
            crate::ids::native_stream(&candidate.id) == Some(stream)
                && candidate.component_record_index > space.component_record_index
        })
        .map(|candidate| candidate.component_record_index)
        .min();
    let mut blobs = HashSet::new();
    for blob in body_bindings
        .iter()
        .filter(|binding| {
            crate::ids::native_stream(binding.id()) == Some(stream)
                && binding.entity_suffix >= space.component_record_index
                && cluster_end.is_none_or(|end| binding.entity_suffix < end)
        })
        .map(crate::records::bodies::DesignBodyBinding::blob_name)
    {
        decode.insert_hash_set(&mut blobs, blob, "index F3D component history blobs")?;
    }
    let mut selected = Vec::new();
    for history in histories {
        let Some(encoded_basename) = crate::ids::native_stream(&history.id)
            .and_then(|stream| stream.strip_prefix(crate::ids::SCHEME_PREFIX))
            .and_then(|entry| entry.rsplit('/').next())
        else {
            continue;
        };
        let mut matches_blob = false;
        for blob in &blobs {
            {
                let ctx = decode;
                ctx.charge_work(1, "match F3D component history blobs")?;
            }
            if crate::ids::encoded_identity_key_component_matches(encoded_basename, blob) {
                matches_blob = true;
                break;
            }
        }
        if matches_blob {
            decode.reserve_vec(&mut selected, 1, "collect F3D component histories")?;
            selected.push(history);
        }
    }
    decode.stable_sort_by(
        &mut selected,
        |left, right| left.id.cmp(&right.id),
        |history| history.id.len(),
        "sort F3D component histories",
    )?;
    selected.dedup_by(|left, right| left.id == right.id);
    Ok(Some(selected))
}

pub(crate) fn historical_extrude_selection_identity_kind(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    member: &DesignExtrudeSelectionMember,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    match component_histories(
        decode,
        member.context_id.as_str(),
        naming_spaces,
        body_bindings,
        histories,
    )? {
        Some(selected) => HistoricalIdentityIndex::build(decode, selected, [member.local_id])?
            .selection_identity_kind(decode, member.local_id),
        None => historical_selection_identity_kind(decode, histories, member.local_id),
    }
}

pub(crate) fn bind_extrude_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    members: &mut [DesignExtrudeSelectionMember],
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for member in members {
        member.historical = None;
        if let Some((kind, entity_ref, states)) = historical_extrude_selection_identity_kind(
            decode,
            member,
            naming_spaces,
            body_bindings,
            histories,
        )? {
            member.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids: states,
            });
        }
    }
    Ok(())
}

/// Resolve both identities in nested entity-selection operands against the
/// owning feature's exact input topology.
pub(crate) fn bind_entity_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let identities = HistoricalIdentityIndex::build(
        decode,
        histories,
        operands.iter().flat_map(|operand| {
            std::iter::once(operand.primary_identity).chain(
                operand
                    .secondary()
                    .map(|secondary| secondary.identity.value),
            )
        }),
    )?;
    for operand in operands {
        operand.historical_edge_candidates.clear();
        operand.historical_face_candidates.clear();
        operand.resolved_edge_slot = None;
        operand.historical_face_candidates =
            entity_selection_face_candidates(decode, operand.primary_identity, histories)?;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let mut matching_states = histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| state.state_id == previous_state_id);
        let Some(state) = matching_states.next() else {
            continue;
        };
        if matching_states.next().is_some() {
            continue;
        }
        let Some(topology) = state.topology() else {
            continue;
        };
        let selections = std::iter::once((0, operand.primary_identity)).chain(
            operand
                .secondary()
                .map(|secondary| (1, secondary.identity.value)),
        );
        operand.historical_edge_candidates = entity_selection_edge_candidates(
            decode,
            selections,
            previous_state_id,
            &identities,
            topology,
        )?;
        operand.resolved_edge_slot =
            unique_entity_selection_edge(&operand.historical_edge_candidates);
    }
    Ok(())
}

/// Resolve direct persistent face selections carried by Hole constructions.
pub(crate) fn bind_hole_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for scope in scopes {
        let history_state_id = scope.history_state_id();
        let previous_history_state_id = scope.previous_history_state_id();
        let Some(construction) = scope.hole_construction_mut() else {
            continue;
        };
        let Some(selection) = &mut construction.face_selection else {
            continue;
        };
        selection.historical_face_candidates.clear();
        selection.historical_face_candidates =
            entity_selection_face_candidates(decode, selection.primary_identity, histories)?;
        if selection.historical_face_candidates.is_empty() {
            if let Some(candidate) = hole_transition_face_candidate(
                decode,
                selection,
                construction.position.map(FiniteReal::get),
                construction.direction.map(FiniteReal::get),
                history_state_id,
                previous_history_state_id,
                histories,
            )? {
                decode.reserve_vec(
                    &mut selection.historical_face_candidates,
                    1,
                    "collect F3D hole support candidates",
                )?;
                selection.historical_face_candidates.push(candidate);
            }
        }
    }
    Ok(())
}

/// Resolve the support face of an edge-backed Hole selection from its exact
/// feature transition. Fusion can serialize the support selector as an edge
/// even when the operation changes a planar support face: the transition then
/// inserts the drill cylinder and updates the support boundary. The admission
/// requires one coaxial inserted cylinder and one updated preceding plane whose
/// oriented normal agrees with the Hole direction. Generic edge-to-face
/// selection remains intentionally ambiguous outside this Hole-specific proof.
fn hole_transition_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &crate::records::feature::hole::DesignHoleFaceSelection,
    position: [f64; 3],
    direction: [f64; 3],
    state_id: Option<i64>,
    previous_state_id: Option<i64>,
    histories: &[AsmHistory],
) -> Result<
    Option<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::{
        body_recipe::AsmHistoricalEntityKind, entity_selection::DesignEntitySelectionFaceCandidate,
    };
    macro_rules! some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    let primary_identity = selection.primary_identity;
    let secondary_identity = selection
        .secondary
        .map(|secondary| secondary.identity.value);

    if secondary_identity.is_some() {
        return Ok(None);
    }
    let (state_id, previous_state_id) = (some!(state_id), some!(previous_state_id));
    let (kind, entity_ref, identity_states) = some!(historical_selection_identity_kind(
        decode,
        histories,
        primary_identity
    )?);
    if kind != AsmHistoricalEntityKind::Edge || !identity_states.contains(&previous_state_id) {
        return Ok(None);
    }
    let (history, result_state, preceding_state) = some!(unique_history_state_pair(
        histories,
        state_id,
        previous_state_id
    ));
    let transition = some!(result_state.transition.as_ref());
    if transition.previous_state_id != Some(previous_state_id) {
        return Ok(None);
    }
    let result_topology = some!(result_state.topology());
    let preceding_topology = some!(preceding_state.topology());
    let point = cadmpeg_ir::math::Point3::new(position[0], position[1], position[2]);
    if position.iter().any(|coordinate| !coordinate.is_finite()) {
        return Ok(None);
    }
    let direction = cadmpeg_ir::math::Vector3::new(direction[0], direction[1], direction[2]);
    let direction = some!(direction.unit());

    let mut cylinder_surfaces = HashSet::new();
    let mut cylinders = Vec::new();
    for face in &transition.topology.faces.inserted {
        let mut bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == *face);
        let Some(binding) = bindings.next() else {
            continue;
        };
        if bindings.next().is_some()
            || !decode.insert_hash_set(
                &mut cylinder_surfaces,
                binding.carrier,
                "index F3D Hole cylinder surfaces",
            )?
        {
            continue;
        }
        let mut carriers = result_topology
            .surface_cylinders
            .iter()
            .filter(|surface| surface.surface == binding.carrier);
        let Some(cylinder) = carriers.next() else {
            continue;
        };
        if carriers.next().is_some() {
            return Ok(None);
        }

        decode.reserve_vec(&mut cylinders, 1, "collect F3D Hole cylinders")?;
        cylinders.push(cylinder);
    }
    if cylinders.is_empty()
        || cylinders.iter().any(|cylinder| {
            let axis = cylinder.axis.unit();
            axis.is_none_or(|axis| !same_axis_line((cylinder.origin, axis), (point, direction)))
        })
    {
        return Ok(None);
    }

    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D Hole preceding faces",
        )?;
    }
    let scale = [
        point.x.abs(),
        point.y.abs(),
        point.z.abs(),
        preceding_topology
            .surface_planes
            .iter()
            .flat_map(|plane| {
                [
                    plane.origin.x.abs(),
                    plane.origin.y.abs(),
                    plane.origin.z.abs(),
                ]
            })
            .fold(0.0, f64::max),
    ]
    .into_iter()
    .fold(1.0, f64::max);
    let mut candidates = decode.collect_vec(
        transition
            .topology
            .faces
            .updated
            .iter()
            .copied()
            .filter(|face| preceding_faces.contains(face))
            .filter_map(|face| {
                let mut bindings = preceding_topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == face);
                let binding = bindings.next()?;
                if bindings.next().is_some() {
                    return None;
                }
                let mut planes = preceding_topology
                    .surface_planes
                    .iter()
                    .filter(|plane| plane.surface == binding.carrier);
                let plane = planes.next()?;
                if planes.next().is_some() {
                    return None;
                }
                let normal = plane.normal.unit()?;
                let point_distance = point.vector_from(plane.origin).dot(normal).abs();
                ((normal.dot(direction) - 1.0).abs() <= HOLE_SUPPORT_NORMAL_TOLERANCE
                    && point_distance <= HOLE_SUPPORT_POINT_TOLERANCE * scale)
                    .then_some(face)
            }),
        "collect F3D Hole transition faces",
    )?;
    decode.sort_unstable_by(
        &mut candidates,
        Ord::cmp,
        |_| 0,
        "sort F3D Hole transition faces",
    )?;
    candidates.dedup();
    let [face_slot] = candidates.as_slice() else {
        return Ok(None);
    };
    let history_id = {
        let ctx = decode;
        ctx.copy_retained_text(&history.id, "copy F3D Hole history identity")?
    };
    Ok(Some(DesignEntitySelectionFaceCandidate {
        history_id,
        historical: crate::records::topology::fillet::HistoricalBinding {
            kind,
            entity_ref,
            state_ids: vec![previous_state_id],
        },
        face_slot: *face_slot,
    }))
}

/// Resolve persistent circular-pattern axis identities in the feature input topology.
pub(crate) fn bind_circular_pattern_axes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::feature::patterns::DesignCircularPatternAxis;
    for scope in scopes {
        let mut matching_histories = histories.iter().filter(|history| {
            scope_histories
                .get(&scope.id)
                .is_some_and(|id| history.id == *id)
                || (scope_histories.get(&scope.id).is_none() && histories.len() == 1)
        });
        let Some(history) = matching_histories.next() else {
            continue;
        };
        if matching_histories.next().is_some() {
            continue;
        }
        let input_state_id =
            effective_scope_previous_history_state_id(scope, std::slice::from_ref(history));
        let Some(construction) = scope.circular_pattern_construction_mut() else {
            continue;
        };
        let DesignCircularPatternAxis::HistoricalEdge {
            persistent_identity,
            resolved,
            ..
        } = &mut construction.axis
        else {
            continue;
        };
        *resolved = None;
        let identities = HistoricalIdentityIndex::build(
            decode,
            std::slice::from_ref(history),
            [*persistent_identity],
        )?;
        let mut axes = historical_pattern_identity_axes(
            decode,
            *persistent_identity,
            &identities,
            history,
            input_state_id,
        )?
        .into_iter();
        let Some(axis) = axes.next() else {
            continue;
        };
        let axis_line = (axis.origin.get(), *axis.direction.as_raw());
        if axes.any(|candidate| {
            !same_axis_line(
                axis_line,
                (candidate.origin.get(), *candidate.direction.as_raw()),
            )
        }) {
            continue;
        }
        *resolved = Some(axis);
    }
    Ok(())
}

pub(super) fn historical_pattern_identity_axes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    identity: u64,
    identities: &HistoricalIdentityIndex,
    history: &AsmHistory,
    input_state_id: Option<i64>,
) -> Result<Vec<crate::records::feature::patterns::DesignAxis>, cadmpeg_core::CodecError> {
    if let Some((kind, entity_ref, state_ids)) =
        identities.selection_identity_kind(decode, identity)?
    {
        let state_ids = if let Some(input_state_id) = input_state_id {
            if !state_ids.contains(&input_state_id) {
                return Ok(Vec::new());
            }
            vec![input_state_id]
        } else {
            state_ids
        };
        return historical_pattern_identity_axes_for_selection(
            decode,
            Some((kind, entity_ref, &state_ids)),
            history,
        );
    }
    let Some(revision) = snapshot_edge_identity_revision(identity, history) else {
        return Ok(Vec::new());
    };
    let archived =
        HistoricalIdentityIndex::build(decode, std::slice::from_ref(history), [revision])?;
    let Some((kind, entity_ref, state_ids)) = archived.selection_identity_kind(decode, revision)?
    else {
        return Ok(Vec::new());
    };
    let state_ids = if let Some(input_state_id) = input_state_id {
        if !state_ids.contains(&input_state_id) {
            return Ok(Vec::new());
        }
        vec![input_state_id]
    } else {
        state_ids
    };
    historical_pattern_identity_axes_for_selection(
        decode,
        Some((kind, entity_ref, &state_ids)),
        history,
    )
}

pub(super) fn snapshot_edge_identity_revision(identity: u64, history: &AsmHistory) -> Option<u64> {
    let mut matches = history
        .states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| match &record.framing {
            crate::history_records::AsmHistoryRecordFraming::Framed { index, .. } => {
                *index == identity
            }
            crate::history_records::AsmHistoryRecordFraming::Opaque { .. } => identity == 0,
        });
    let record = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    (record.name() == "edge")
        .then_some(record.revision_id?)
        .filter(|revision| *revision > 0)
        .and_then(|revision| u64::try_from(revision).ok())
}

pub(super) fn historical_pattern_identity_axes_for_selection(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selected: Option<(AsmHistoricalEntityKind, i64, &[i64])>,
    history: &AsmHistory,
) -> Result<Vec<crate::records::feature::patterns::DesignAxis>, cadmpeg_core::CodecError> {
    let Some((kind, entity_ref, state_ids)) = selected else {
        return Ok(Vec::new());
    };
    if state_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut axes = Vec::new();
    let mut matched_state_ids = HashSet::new();
    for state in history
        .states
        .iter()
        .filter(|state| state_ids.contains(&state.state_id))
    {
        decode.insert_hash_set(
            &mut matched_state_ids,
            state.state_id,
            "index F3D pattern axis states",
        )?;
        let Some(topology) = state.topology() else {
            return Ok(Vec::new());
        };
        let state_axes = decode.collect_vec(
            historical_pattern_identity_axis_candidates(
                decode,
                Some((kind, entity_ref)),
                topology,
            )?
            .into_iter()
            .filter_map(|(origin, direction)| design_axis(origin, direction)),
            "collect F3D pattern state axes",
        )?;
        if state_axes.is_empty() {
            return Ok(Vec::new());
        }
        for axis in state_axes {
            decode.reserve_vec(&mut axes, 1, "collect F3D pattern axes")?;
            axes.push(axis);
        }
    }
    if matched_state_ids.len() != state_ids.len() || axes.is_empty() {
        return Ok(Vec::new());
    }
    Ok(axes)
}

fn historical_pattern_identity_axis_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selected: Option<(AsmHistoricalEntityKind, i64)>,
    topology: &AsmHistoricalTopology,
) -> Result<Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError> {
    let Some((kind, entity_ref)) = selected else {
        return Ok(Vec::new());
    };
    match kind {
        AsmHistoricalEntityKind::Face => decode.collect_vec(
            historical_face_surface_axis(entity_ref, topology),
            "collect F3D face axis candidates",
        ),
        AsmHistoricalEntityKind::Surface => decode.collect_vec(
            historical_surface_axis(entity_ref, topology),
            "collect F3D surface axis candidates",
        ),
        _ => {
            let mut axes = Vec::new();
            for edge in historical_identity_edges(decode, kind, entity_ref, topology)? {
                let Some(axis) = historical_edge_axis(decode, edge, topology)? else {
                    continue;
                };

                decode.reserve_vec(&mut axes, 1, "collect F3D edge axis candidates")?;
                axes.push(axis);
            }
            Ok(axes)
        }
    }
}

fn historical_face_surface_axis(
    face: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let binding = bindings.next()?;
    bindings
        .next()
        .is_none()
        .then(|| historical_surface_axis(binding.carrier, topology))?
}

fn historical_surface_axis(
    surface: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let mut candidates = topology
        .surface_axes
        .iter()
        .filter(|axis| axis.surface == surface)
        .map(|axis| (axis.origin, axis.direction))
        .chain(
            topology
                .surface_planes
                .iter()
                .filter(|plane| plane.surface == surface)
                .map(|plane| (plane.origin, plane.normal)),
        );
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Bind persistent Mirror plane selections to exact planes in the selected
/// historical topology.
pub(crate) fn bind_mirror_selection_planes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for scope in scopes {
        let stream = crate::ids::native_stream(&scope.id).map(str::to_owned);
        let record_index = scope.record_index;
        let history_state_id = scope.history_state_id();
        let previous_history_state_id = scope.previous_history_state_id();
        let Some(construction) = scope.mirror_construction_mut() else {
            continue;
        };
        construction.plane = None;
        let (Some(selection_record_index), Some(state_id), Some(previous_state_id)) = (
            construction.plane_selection_record_index,
            history_state_id,
            previous_history_state_id,
        ) else {
            continue;
        };
        let Some((history, _, _)) =
            unique_history_state_pair(histories, state_id, previous_state_id)
        else {
            continue;
        };
        let mut matching_groups = groups.iter().filter(|group| {
            crate::ids::native_stream(&group.id) == stream.as_deref()
                && group.scope_record_index == record_index
                && group.record_index == construction.plane_group_record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([selection_record_index])
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut matching_operands = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream.as_deref()
                && operand.scope_record_index == record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index() == selection_record_index
        });
        let matching_operand = matching_operands.next();
        if matching_operands.next().is_some() {
            continue;
        }
        let mut matching_face_operands = face_operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream.as_deref()
                && operand.scope_record_index == record_index
                && operand.group_record_index() == Some(group.record_index)
                && operand.group_member_ordinal() == Some(0)
                && operand.record_index() == selection_record_index
        });
        let matching_face_operand = matching_face_operands.next();
        if matching_face_operands.next().is_some() {
            continue;
        }
        let plane = if let Some(operand) = matching_operand {
            if matching_face_operand.is_some() {
                continue;
            }
            let mut matching_identities = identities.iter().filter(|identity| {
                crate::ids::native_stream(&identity.id) == stream.as_deref()
                    && identity.group_record_index == group.record_index
            });
            let identity = matching_identities.next();
            if matching_identities.next().is_some() {
                continue;
            }
            let persistent_candidates = if let Some(identity) = identity
                .and_then(crate::records::topology::construction::DesignConstructionOperandIdentity::persistent_identity) {
                entity_selection_face_candidates(decode, identity.local_id, histories)?
            } else {
                Vec::new()
            };
            let primary_candidates =
                entity_selection_face_candidates(decode, operand.primary_identity, histories)?;
            if primary_candidates.is_empty() && persistent_candidates.is_empty() {
                design_geometry_mirror_plane(operand.primary_identity)
            } else {
                let Some(candidate) = unique_mirror_plane_candidate(
                    decode,
                    primary_candidates,
                    persistent_candidates,
                )?
                else {
                    continue;
                };
                historical_mirror_plane(decode, &candidate, previous_state_id, histories)?
            }
        } else if let Some(operand) = matching_face_operand {
            historical_mirror_face_operand_plane(operand, history, previous_state_id)
        } else {
            continue;
        };
        let Some(plane) = plane else { continue };
        let norm = (plane.normal.x * plane.normal.x
            + plane.normal.y * plane.normal.y
            + plane.normal.z * plane.normal.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9 {
            continue;
        }
        let (Some(origin), Some(normal)) = (
            cadmpeg_ir::features::FinitePoint3::new(plane.origin),
            cadmpeg_ir::features::FiniteVector3::new(plane.normal),
        ) else {
            continue;
        };
        construction.plane =
            crate::records::feature::patterns::DesignPlane::from_parts(origin, normal);
    }
    Ok(())
}

pub(super) fn historical_mirror_face_operand_plane(
    operand: &crate::records::topology::face::DesignFaceOperand,
    history: &AsmHistory,
    previous_state_id: i64,
) -> Option<HistoricalMirrorPlane> {
    if !operand.resolved_face_slots.is_empty() {
        return coincident_mirror_plane(
            operand.resolved_face_slots.iter().copied(),
            previous_state_id,
            history,
        );
    }
    if !operand.preceding_candidate_faces.is_empty() {
        return coincident_mirror_plane(
            operand
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str())),
            previous_state_id,
            history,
        );
    }
    let referenced = || {
        operand.recipe_references.iter().flat_map(|reference| {
            reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces)
        })
    };
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face
        && referenced().next().is_some()
    {
        return coincident_mirror_plane(
            referenced().filter_map(|face| stable_ref(face.as_str())),
            previous_state_id,
            history,
        );
    }
    coincident_mirror_plane(
        crate::design::face_resolve::face_operand_candidates(operand)
            .iter()
            .filter_map(|face| stable_ref(face.as_str())),
        previous_state_id,
        history,
    )
}

fn coincident_mirror_plane(
    slots: impl Iterator<Item = i64> + Clone,
    previous_state_id: i64,
    history: &AsmHistory,
) -> Option<HistoricalMirrorPlane> {
    let first_slot = slots.clone().min()?;
    let first = historical_mirror_plane_for_face_slot(first_slot, previous_state_id, history)?;
    for slot in slots {
        let candidate = historical_mirror_plane_for_face_slot(slot, previous_state_id, history)?;
        if !mirror_planes_coincident(&first, &candidate) {
            return None;
        }
    }
    Some(first)
}

pub(super) fn unique_mirror_plane_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    mut primary: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
    mut persistent: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
) -> Result<
    Option<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    decode.stable_sort_by(
        &mut primary,
        |left, right| left.history_id.cmp(&right.history_id),
        |candidate| candidate.history_id.len(),
        "f3d mirror plane primary candidates sort",
    )?;
    primary.dedup();
    persistent.retain(|candidate| {
        primary
            .iter()
            .any(|context| context.history_id == candidate.history_id)
    });
    decode.stable_sort_by(
        &mut persistent,
        |left, right| left.history_id.cmp(&right.history_id),
        |candidate| candidate.history_id.len(),
        "f3d mirror plane persistent candidates sort",
    )?;
    persistent.dedup();
    Ok(match persistent.len() {
        1 => persistent.pop(),
        0 if primary.len() == 1 => primary.pop(),
        _ => None,
    })
}

#[derive(Clone)]
pub(super) struct HistoricalMirrorPlane {
    pub(super) origin: cadmpeg_ir::math::Point3,
    pub(super) normal: cadmpeg_ir::math::Vector3,
}

/// Resolve the three built-in Design Geometry origin planes.
///
/// These identities are outside the ASM history namespace. A numeric identity
/// is admitted here only after both the primary and persistent history lookups
/// have found no candidate, so a topology identity with the same value keeps
/// the normal historical-resolution path.
pub(super) fn design_geometry_mirror_plane(primary_identity: u64) -> Option<HistoricalMirrorPlane> {
    let normal = match primary_identity {
        42 => cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        43 => cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        44 => cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        _ => return None,
    };
    Some(HistoricalMirrorPlane {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        normal,
    })
}

pub(super) fn historical_mirror_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    preferred_state_id: i64,
    histories: &[AsmHistory],
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    if candidate.historical.state_ids.contains(&preferred_state_id) {
        return historical_mirror_plane_in_state(decode, candidate, preferred_state_id, histories);
    }
    let mut resolved = None;
    for state_id in &candidate.historical.state_ids {
        let Some(plane) =
            historical_mirror_plane_in_state(decode, candidate, *state_id, histories)?
        else {
            return Ok(None);
        };
        if resolved
            .as_ref()
            .is_some_and(|exact: &HistoricalMirrorPlane| !mirror_planes_coincident(exact, &plane))
        {
            return Ok(None);
        }
        resolved = Some(plane);
    }
    Ok(resolved)
}

fn mirror_planes_coincident(left: &HistoricalMirrorPlane, right: &HistoricalMirrorPlane) -> bool {
    let dot = left.normal.dot(right.normal);
    let normal_distance = right.origin.vector_from(left.origin).dot(left.normal);
    (dot.abs() - 1.0).abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9
        && normal_distance.abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8
}

fn historical_mirror_plane_in_state(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    state_id: i64,
    histories: &[AsmHistory],
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut matching_histories = histories
        .iter()
        .filter(|history| history.id == candidate.history_id);
    let Some(history) = matching_histories.next() else {
        return Ok(None);
    };
    if matching_histories.next().is_some() {
        return Ok(None);
    }
    let mut matching_states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let Some(state) = matching_states.next() else {
        return Ok(None);
    };
    if matching_states.next().is_some() {
        return Ok(None);
    }
    let Some(topology) = state.topology() else {
        return Ok(None);
    };
    match candidate.historical.kind {
        AsmHistoricalEntityKind::Coedge => {
            historical_mirror_coedge_plane(decode, candidate.historical.entity_ref, topology)
        }
        AsmHistoricalEntityKind::Loop => {
            historical_loop_plane(decode, candidate.historical.entity_ref, topology)
        }
        _ => Ok(historical_mirror_plane_for_face_slot_in_topology(
            candidate.face_slot,
            topology,
        )),
    }
}

fn historical_mirror_plane_for_face_slot(
    face_slot: i64,
    state_id: i64,
    history: &AsmHistory,
) -> Option<HistoricalMirrorPlane> {
    let mut matching_states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = matching_states.next()?;
    if matching_states.next().is_some() {
        return None;
    }
    let topology = state.topology()?;
    historical_mirror_plane_for_face_slot_in_topology(face_slot, topology)
}

fn historical_mirror_plane_for_face_slot_in_topology(
    face_slot: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face_slot);
    let binding = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|plane| plane.surface == binding.carrier);
    let plane = planes.next()?;
    planes.next().is_none().then_some(HistoricalMirrorPlane {
        origin: plane.origin,
        normal: plane.normal,
    })
}

macro_rules! mirror_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

pub(super) fn historical_loop_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    loop_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut loop_relations = topology
        .loop_coedges
        .iter()
        .filter(|relation| relation.owner_ref == loop_ref);
    let relation = mirror_some!(loop_relations.next());
    if loop_relations.next().is_some() || relation.member_refs.is_empty() {
        return Ok(None);
    }
    let mut planes = Vec::new();
    for coedge_ref in &relation.member_refs {
        let mut coedges = topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == *coedge_ref && coedge.owner_loop == loop_ref);
        let coedge = mirror_some!(coedges.next());
        if coedges.next().is_some() {
            return Ok(None);
        }
        let mut bindings = topology
            .edge_curves
            .iter()
            .filter(|binding| binding.entity == coedge.edge);
        let binding = mirror_some!(bindings.next());
        if bindings.next().is_some() {
            return Ok(None);
        }
        let curve = mirror_some!(binding.carrier);
        let mut axes = topology
            .curve_axes
            .iter()
            .filter(|axis| axis.curve == curve);
        let axis = mirror_some!(axes.next());
        if axes.next().is_some() {
            return Ok(None);
        }
        let norm = (axis.direction.x * axis.direction.x
            + axis.direction.y * axis.direction.y
            + axis.direction.z * axis.direction.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9 {
            return Ok(None);
        }

        decode.reserve_vec(&mut planes, 1, "collect F3D loop mirror planes")?;
        planes.push(HistoricalMirrorPlane {
            origin: axis.origin,
            normal: axis.direction,
        });
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return Ok(None);
    };
    Ok(remaining
        .iter()
        .all(|candidate| mirror_planes_coincident(first, candidate))
        .then_some(first.clone()))
}

/// Resolve a Mirror plane from a selected coedge's complete radial cycle.
///
/// A coedge selects a boundary occurrence, not a face. Its owning face can be
/// non-planar while another face incident to the same topological edge is the
/// selected plane. The cycle and relation checks keep that inference exact:
/// every coedge for the edge must participate in one closed radial cycle, each
/// cycle member must belong to one loop and one face, and the incident faces
/// must expose one coincident set of exact plane carriers.
pub(super) fn historical_mirror_coedge_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    coedge_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut coedges = HashMap::new();
    for coedge in &topology.coedge_topology {
        if !coedges.contains_key(&coedge.coedge) {
            decode.reserve_map(&mut coedges, 1, "index F3D mirror coedges")?;
        }
        if coedges.insert(coedge.coedge, coedge).is_some() {
            return Ok(None);
        }
    }
    let selected = mirror_some!(coedges.get(&coedge_ref));
    let edge = selected.edge;
    let mut same_edge = HashSet::new();
    for coedge in coedges.values().filter(|coedge| coedge.edge == edge) {
        decode.insert_hash_set(
            &mut same_edge,
            coedge.coedge,
            "index F3D mirror edge coedges",
        )?;
    }
    let mut radial_cycle = Vec::new();
    let mut current = coedge_ref;
    loop {
        if current == coedge_ref && !radial_cycle.is_empty() {
            break;
        }
        if radial_cycle.contains(&current) {
            return Ok(None);
        }
        let coedge = mirror_some!(coedges.get(&current));
        if coedge.edge != edge {
            return Ok(None);
        }

        decode.reserve_vec(&mut radial_cycle, 1, "collect F3D mirror radial cycle")?;
        radial_cycle.push(current);
        current = coedge.radial_next;
    }
    let mut radial_refs = HashSet::new();
    for coedge in &radial_cycle {
        decode.insert_hash_set(&mut radial_refs, *coedge, "index F3D mirror radial refs")?;
    }
    if radial_refs != same_edge {
        return Ok(None);
    }

    let mut faces = HashSet::new();
    for coedge_ref in radial_cycle {
        let coedge = mirror_some!(coedges.get(&coedge_ref));
        let mut loop_relations = topology.loop_coedges.iter().filter(|relation| {
            relation.owner_ref == coedge.owner_loop && relation.member_refs.contains(&coedge_ref)
        });
        let loop_relation = mirror_some!(loop_relations.next());
        if loop_relations.next().is_some()
            || loop_relation
                .member_refs
                .iter()
                .filter(|member| **member == coedge_ref)
                .count()
                != 1
        {
            return Ok(None);
        }
        let mut face_relations = topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&coedge.owner_loop));
        let face_relation = mirror_some!(face_relations.next());
        if face_relations.next().is_some()
            || face_relation
                .member_refs
                .iter()
                .filter(|member| **member == coedge.owner_loop)
                .count()
                != 1
        {
            return Ok(None);
        }
        decode.insert_hash_set(
            &mut faces,
            face_relation.owner_ref,
            "index F3D mirror incident faces",
        )?;
    }

    let mut planes = Vec::new();
    for face in faces {
        let mut surface_bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let surface = mirror_some!(surface_bindings.next());
        if surface_bindings.next().is_some() {
            return Ok(None);
        }
        let mut surface_planes = topology
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == surface.carrier);
        if let Some(plane) = surface_planes.next() {
            if surface_planes.next().is_some() {
                return Ok(None);
            }

            decode.reserve_vec(&mut planes, 1, "collect F3D coedge mirror planes")?;
            planes.push(HistoricalMirrorPlane {
                origin: plane.origin,
                normal: plane.normal,
            });
        }
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return Ok(None);
    };
    Ok(remaining
        .iter()
        .all(|candidate| mirror_planes_coincident(first, candidate))
        .then_some(first.clone()))
}

pub(super) fn entity_selection_face_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    local_id: u64,
    histories: &[AsmHistory],
) -> Result<
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;

    let mut candidates = Vec::new();
    'histories: for history in histories {
        let identities =
            HistoricalIdentityIndex::build(decode, std::slice::from_ref(history), [local_id])?;
        let Some((kind, entity_ref, state_ids)) =
            identities.selection_identity_kind(decode, local_id)?
        else {
            continue;
        };
        let mut face_slot = None;
        for state_id in &state_ids {
            let mut states = history
                .states
                .iter()
                .filter(|state| state.state_id == *state_id);
            let Some(state) = states.next() else {
                continue 'histories;
            };
            if states.next().is_some() {
                continue 'histories;
            }
            let Some(topology) = state.topology() else {
                continue 'histories;
            };
            let mut faces =
                historical_identity_faces(decode, kind, entity_ref, topology)?.into_iter();
            let Some(state_face) = faces.next() else {
                continue 'histories;
            };
            if faces.next().is_some() || face_slot.is_some_and(|face| face != state_face) {
                continue 'histories;
            }
            face_slot = Some(state_face);
        }
        let Some(face_slot) = face_slot else {
            continue;
        };
        let history_id = {
            let ctx = decode;
            ctx.copy_retained_text(&history.id, "copy F3D selection history ID")?
        };

        decode.reserve_vec(&mut candidates, 1, "collect F3D selection face candidates")?;
        candidates.push(DesignEntitySelectionFaceCandidate {
            history_id,
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids,
            },
            face_slot,
        });
    }
    Ok(candidates)
}

pub(super) fn entity_selection_edge_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    identities: impl IntoIterator<Item = (u32, u64)>,
    previous_state_id: i64,
    history_identities: &HistoricalIdentityIndex,
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate;

    let mut candidates = Vec::new();
    for (identity_ordinal, local_id) in identities {
        let Some((kind, entity_ref, states)) =
            history_identities.selection_identity_kind(decode, local_id)?
        else {
            continue;
        };
        if !states.contains(&previous_state_id) {
            continue;
        }
        let mut edge_slots = decode.collect_vec(
            historical_identity_edges(decode, kind, entity_ref, topology)?,
            "collect F3D selected identity edges",
        )?;
        decode.sort_unstable_by(
            &mut edge_slots,
            Ord::cmp,
            |_| 0,
            "sort F3D selected identity edges",
        )?;
        if edge_slots.is_empty() {
            continue;
        }

        decode.reserve_vec(&mut candidates, 1, "collect F3D selection edge candidates")?;
        candidates.push(DesignEntitySelectionEdgeCandidate {
            identity_ordinal,
            local_id,
            historical_entity_kind: kind,
            historical_entity_ref: entity_ref,
            edge_slots,
        });
    }
    Ok(candidates)
}

pub(super) fn unique_entity_selection_edge(
    candidates: &[crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate],
) -> Option<i64> {
    let first = candidates.first()?;
    let mut unique = None;
    for edge in &first.edge_slots {
        if candidates[1..]
            .iter()
            .all(|candidate| candidate.edge_slots.contains(edge))
        {
            if unique.is_some_and(|prior| prior != *edge) {
                return None;
            }
            unique = Some(*edge);
        }
    }
    unique
}

pub(crate) fn bind_edge_identity_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [DesignEdgeIdentityOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    struct EdgeTreatmentTransitionCandidates {
        radii: Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        treatment_edges: Vec<i64>,
        deleted_edges: Vec<i64>,
    }

    if projection_was_finalized(histories) {
        return Ok(());
    }
    let mut compact_group_counts = HashMap::<(String, u32, u32), Option<usize>>::new();
    for operand in operands.iter() {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let stream = decode.copy_retained_text(stream, "copy F3D compact edge group stream")?;
        let key = (
            stream,
            operand.scope_record_index,
            operand.group_record_index,
        );
        if !compact_group_counts.contains_key(&key) {
            decode.reserve_map(
                &mut compact_group_counts,
                1,
                "index F3D compact edge groups",
            )?;
        }
        compact_group_counts
            .entry(key)
            .and_modify(|count| {
                *count = count.and_then(|count| operand.layout().is_compact().then_some(count + 1));
            })
            .or_insert(operand.layout().is_compact().then_some(1));
    }
    let local_ids = decode.collect_vec(
        operands
            .iter()
            .map(|operand| operand.local_id)
            .chain(identities.iter().filter_map(|identity| {
                identity
                    .persistent_identity()
                    .map(|persistent| persistent.local_id)
            })),
        "collect F3D edge identity local ids",
    )?;
    let history_identities =
        HistoricalIdentityIndex::build(decode, histories, local_ids.iter().copied())?;
    let mut identities_by_history = HashMap::new();
    for history in histories {
        let index = HistoricalIdentityIndex::build(
            decode,
            std::slice::from_ref(history),
            local_ids.iter().copied(),
        )?;

        decode.reserve_map(
            &mut identities_by_history,
            1,
            "index F3D scoped history identities",
        )?;
        identities_by_history.insert(history.id.as_str(), index);
    }
    let mut treatment_cache = super::treatment_index::TreatmentCache::default();
    let mut treatment_candidates_by_transition =
        HashMap::<(String, i64, i64), EdgeTreatmentTransitionCandidates>::new();
    for operand in operands {
        operand.historical = None;
        operand.treatment_radius_candidates.clear();
        operand.transition_edge_candidates.clear();
        operand.resolved_edge_slots.clear();
        operand.resolved_edge_slot = None;
        operand.resolution_identity_id = None;
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| {
            crate::ids::native_stream(&scope.id) == Some(stream)
                && scope.record_index == operand.scope_record_index
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let current_state_id = scope.history_state_id();
        let bound_history = bound_scope_history(&scope.id, scope_histories, histories);
        let scoped_identities = bound_history
            .and_then(|history| identities_by_history.get(history.id.as_str()))
            .unwrap_or(&history_identities);
        if let Some((kind, entity_ref, states)) = scoped_identities
            .selection_identity_kind(decode, operand.local_id)?
            .filter(|(_, _, states)| states.contains(&previous_state_id))
        {
            operand.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids: states,
            });
        }
        let Some(history) = bound_history else {
            continue;
        };
        let mut previous_states = history
            .states
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous_state) = previous_states.next() else {
            continue;
        };
        if previous_states.next().is_some() {
            continue;
        }
        let Some(topology) = previous_state.topology() else {
            continue;
        };
        if let Some(current_state_id) = current_state_id {
            let mut current_states = history
                .states
                .iter()
                .filter(|state| state.state_id == current_state_id);
            let current_state = current_states.next();
            if current_states.next().is_none()
                && current_state
                    .is_some_and(|state| history_state_reaches(history, state, previous_state_id))
            {
                let key = (
                    decode.copy_retained_text(
                        &history.id,
                        "copy F3D treatment transition history id",
                    )?,
                    current_state_id,
                    previous_state_id,
                );
                if !treatment_candidates_by_transition.contains_key(&key) {
                    if let Some(result) = current_state.and_then(|state| state.topology()) {
                        let mut preceding_faces = HashSet::new();
                        for face in &topology.faces {
                            decode.insert_hash_set(
                                &mut preceding_faces,
                                *face,
                                "index F3D treatment preceding faces",
                            )?;
                        }
                        let inserted_faces = decode.collect_vec(
                            result
                                .faces
                                .iter()
                                .copied()
                                .filter(|face| !preceding_faces.contains(face)),
                            "collect F3D treatment inserted faces",
                        )?;
                        let mut result_edges = HashSet::new();
                        for edge in &result.edges {
                            decode.insert_hash_set(
                                &mut result_edges,
                                *edge,
                                "index F3D treatment result edges",
                            )?;
                        }
                        let deleted_edges = decode.collect_vec(
                            topology
                                .edges
                                .iter()
                                .copied()
                                .filter(|edge| !result_edges.contains(edge)),
                            "collect F3D treatment deleted edges",
                        )?;
                        let (radii, treatment_edges) = treatment_edge_candidates(
                            decode,
                            None,
                            &inserted_faces,
                            result,
                            topology,
                            &deleted_edges,
                            &mut treatment_cache,
                        )?;

                        decode.reserve_map(
                            &mut treatment_candidates_by_transition,
                            1,
                            "index F3D treatment transitions",
                        )?;
                        treatment_candidates_by_transition.insert(
                            (
                                decode.copy_retained_text(
                                    &history.id,
                                    "copy F3D treatment transition key",
                                )?,
                                current_state_id,
                                previous_state_id,
                            ),
                            EdgeTreatmentTransitionCandidates {
                                radii,
                                treatment_edges,
                                deleted_edges,
                            },
                        );
                    }
                }
                if let Some(candidates) = treatment_candidates_by_transition.get(&key) {
                    operand.treatment_radius_candidates = decode.collect_vec(
                        candidates.radii.iter().cloned(),
                        "copy F3D treatment radius candidates",
                    )?;
                    let mut treatment_edges = decode.collect_vec(
                        candidates.treatment_edges.iter().copied(),
                        "copy F3D treatment edges",
                    )?;
                    // The geometric chain is transition-scoped. The deleted-
                    // set fallback is group-scoped because its proof depends
                    // on this operand group's compact member count.
                    if treatment_edges.is_empty() {
                        let is_edge_treatment = matches!(
                            crate::design::design_feature_family(&scope.kind()),
                            Some(
                                crate::design::DesignFeatureFamily::Fillet
                                    | crate::design::DesignFeatureFamily::Chamfer
                            )
                        );
                        let compact_member_count = compact_group_counts
                            .get(&(
                                decode.copy_retained_text(
                                    stream,
                                    "copy F3D compact group lookup stream",
                                )?,
                                operand.scope_record_index,
                                operand.group_record_index,
                            ))
                            .copied()
                            .flatten();
                        treatment_edges = complete_compact_edge_treatment_deletions(
                            decode,
                            is_edge_treatment,
                            compact_member_count,
                            &candidates.deleted_edges,
                        )?;
                    }
                    operand.transition_edge_candidates = decode
                        .collect_vec(treatment_edges, "copy F3D transition edge candidates")?;
                }
            }
        }
        let direct = if let Some(binding) = operand
            .historical
            .as_ref()
            .filter(|binding| binding.state_ids.contains(&previous_state_id))
        {
            historical_identity_edge(decode, binding.kind, binding.entity_ref, topology)?
        } else {
            None
        };
        if let Some(edge) = direct {
            operand.resolved_edge_slot = Some(edge);
            operand.resolution_identity_id =
                Some(decode.copy_retained_text(&operand.id, "copy F3D edge resolution identity")?);
            continue;
        }
        let mut resolved = Vec::new();
        for identity in identities.iter().filter(|identity| {
            crate::ids::native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == operand.group_record_index
        }) {
            let Some(persistent) = identity.persistent_identity() else {
                continue;
            };
            let Some((kind, entity_ref, states)) =
                scoped_identities.selection_identity_kind(decode, persistent.local_id)?
            else {
                continue;
            };
            if !states.contains(&previous_state_id) {
                continue;
            }
            let Some(edge) = historical_identity_edge(decode, kind, entity_ref, topology)? else {
                continue;
            };

            decode.reserve_vec(&mut resolved, 1, "collect F3D resolved edge identities")?;
            resolved.push((edge, identity.id.as_str()));
        }
        let mut resolved = resolved.into_iter();
        let Some((edge, identity_id)) = resolved.next() else {
            continue;
        };
        if resolved.any(|candidate| candidate.0 != edge) {
            continue;
        }
        operand.resolved_edge_slot = Some(edge);
        operand.resolution_identity_id =
            Some(decode.copy_retained_text(identity_id, "copy F3D edge resolution identity")?);
    }
    Ok(())
}

pub(super) fn complete_compact_edge_treatment_deletions(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    is_edge_treatment: bool,
    compact_member_count: Option<usize>,
    deleted_edges: &[i64],
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    if is_edge_treatment
        && !deleted_edges.is_empty()
        && compact_member_count == Some(deleted_edges.len())
    {
        decode.collect_vec(
            deleted_edges.iter().copied(),
            "copy F3D compact treatment deletions",
        )
    } else {
        Ok(Vec::new())
    }
}

/// Resolve a class-297 edge-treatment member whose persistent local identity
/// names the member's embedded bounded-face recipe. The rule selects every
/// deleted treatment edge on the recipe's exact preceding support face.
pub(crate) fn bind_edge_identity_bounded_face_rules(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [DesignEdgeIdentityOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::recipes::ConstructionRecipeKind;

    for operand in operands {
        operand.resolved_edge_slots.clear();
        if operand.resolved_edge_slot.is_some() {
            continue;
        }
        let mut matches = face_operands.iter().filter(|face| {
            crate::ids::native_stream(&face.id) == crate::ids::native_stream(&operand.id)
                && face.scope_record_index == operand.scope_record_index
                && face.group_record_index() == Some(operand.group_record_index)
                && face.group_member_ordinal() == Some(operand.group_member_ordinal)
                && face.record_index() == operand.record_index()
                && face.class_tag == operand.class_tag
                && face.recipe_kind == ConstructionRecipeKind::BoundedFace
                && u64::from(face.recipe_record_index()) == operand.local_id
        });
        let Some(face) = matches.next() else { continue };
        if matches.next().is_some() {
            continue;
        }
        let [support] = face.historical_support_contexts.as_slice() else {
            continue;
        };
        let mut unique_faces = HashSet::new();
        for face in &support.preceding_face_slots {
            decode.insert_hash_set(
                &mut unique_faces,
                *face,
                "index F3D bounded treatment faces",
            )?;
        }
        if support.preceding_face_slots.is_empty()
            || support.changed_preceding_face_slots != support.preceding_face_slots
            || support.preceding_face_boundaries.len() != support.preceding_face_slots.len()
            || unique_faces.len() != support.preceding_face_slots.len()
            || support.preceding_face_boundaries.iter().any(|boundary| {
                support
                    .preceding_face_boundaries
                    .iter()
                    .filter(|candidate| candidate.face_slot == boundary.face_slot)
                    .count()
                    != 1
                    || !support.preceding_face_slots.contains(&boundary.face_slot)
                    || boundary
                        .loops
                        .iter()
                        .any(|loop_| loop_.boundary.coedges().next().is_none())
            })
        {
            continue;
        }
        let mut transition = HashSet::new();
        for edge in &operand.transition_edge_candidates {
            decode.insert_hash_set(&mut transition, *edge, "index F3D bounded treatment edges")?;
        }
        if transition.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for edge in support
            .preceding_face_boundaries
            .iter()
            .flat_map(|boundary| &boundary.loops)
            .flat_map(|loop_| loop_.boundary.coedges().map(|row| row.edge_slot))
        {
            if transition.contains(&edge)
                && decode.insert_hash_set(&mut seen, edge, "index F3D bounded resolved edges")?
            {
                decode.reserve_vec(&mut resolved, 1, "collect F3D bounded resolved edges")?;
                resolved.push(edge);
            }
        }
        operand.resolved_edge_slots = resolved;
        if !operand.resolved_edge_slots.is_empty() {
            operand.resolution_identity_id =
                Some(decode.copy_retained_text(&face.id, "copy F3D bounded edge identity")?);
        }
    }
    Ok(())
}

pub(super) fn historical_identity_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let candidates = historical_identity_edges(decode, kind, entity_ref, topology)?;
    let mut candidates = candidates.into_iter();
    let Some(edge) = candidates.next() else {
        return Ok(None);
    };
    Ok(candidates.next().is_none().then_some(edge))
}

fn historical_identity_edges(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let mut candidates = HashSet::new();
    match kind {
        AsmHistoricalEntityKind::Edge => {
            if topology.edges.contains(&entity_ref) {
                decode.insert_hash_set(
                    &mut candidates,
                    entity_ref,
                    "collect F3D identity edges",
                )?;
            }
        }
        AsmHistoricalEntityKind::Coedge => {
            for edge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.coedge == entity_ref)
                .map(|coedge| coedge.edge)
            {
                decode.insert_hash_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(
                    &mut coedges,
                    coedge,
                    "index F3D identity pcurve coedges",
                )?;
            }
            for edge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedges.contains(&coedge.coedge))
                .map(|coedge| coedge.edge)
            {
                decode.insert_hash_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Curve => {
            for edge in topology
                .edge_curves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Vertex | AsmHistoricalEntityKind::Point => {
            let mut vertices = HashSet::new();
            if kind == AsmHistoricalEntityKind::Vertex {
                decode.insert_hash_set(&mut vertices, entity_ref, "index F3D identity vertices")?;
            } else {
                for vertex in topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity)
                {
                    decode.insert_hash_set(&mut vertices, vertex, "index F3D identity vertices")?;
                }
            }
            for edge in topology
                .edge_vertices
                .iter()
                .filter(|edge| {
                    vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                })
                .map(|edge| edge.edge)
            {
                decode.insert_hash_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Face
        | AsmHistoricalEntityKind::Loop
        | AsmHistoricalEntityKind::Surface => {}
    }
    Ok(candidates)
}

fn historical_identity_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let mut carriers = HashSet::new();
    match kind {
        AsmHistoricalEntityKind::Face => {
            decode.insert_hash_set(&mut carriers, entity_ref, "collect F3D identity faces")?;
            return Ok(carriers);
        }
        AsmHistoricalEntityKind::Loop => {
            decode.insert_hash_set(&mut carriers, entity_ref, "index F3D identity face loops")?;
        }
        AsmHistoricalEntityKind::Coedge => {
            for loop_slot in topology
                .loop_coedges
                .iter()
                .filter(|relation| relation.member_refs.contains(&entity_ref))
                .map(|relation| relation.owner_ref)
            {
                decode.insert_hash_set(
                    &mut carriers,
                    loop_slot,
                    "index F3D identity face loops",
                )?;
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(&mut coedges, coedge, "index F3D identity face pcurves")?;
            }
            for loop_slot in topology
                .loop_coedges
                .iter()
                .filter(|relation| {
                    relation
                        .member_refs
                        .iter()
                        .any(|coedge| coedges.contains(coedge))
                })
                .map(|relation| relation.owner_ref)
            {
                decode.insert_hash_set(
                    &mut carriers,
                    loop_slot,
                    "index F3D identity face loops",
                )?;
            }
        }
        AsmHistoricalEntityKind::Surface => {
            for face in topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == entity_ref)
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(&mut carriers, face, "collect F3D identity faces")?;
            }
            return Ok(carriers);
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Edge
        | AsmHistoricalEntityKind::Vertex
        | AsmHistoricalEntityKind::Point
        | AsmHistoricalEntityKind::Curve => return Ok(HashSet::new()),
    }
    let mut faces = HashSet::new();
    for face in topology
        .face_loops
        .iter()
        .filter(|relation| {
            relation
                .member_refs
                .iter()
                .any(|loop_slot| carriers.contains(loop_slot))
        })
        .map(|relation| relation.owner_ref)
    {
        decode.insert_hash_set(&mut faces, face, "collect F3D identity faces")?;
    }
    Ok(faces)
}
