// SPDX-License-Identifier: Apache-2.0
//! Admit historical face references against the input topologies that were emitted.
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::edge_treatments::{FullRoundFilletGroup, FullRoundSideSelection};
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeStart, FaceSelection, Feature, FeatureDefinition, FeatureId,
    FeatureInputTopology, FeatureOperation, LinearTermination,
};
use cadmpeg_ir::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{HashMap, HashSet};

type InputStates<'a> =
    HashMap<(&'a FeatureId, &'a FeatureInputTopologyId), Option<&'a FeatureInputTopology>>;
type FeatureInputs<'a> = HashMap<&'a FeatureId, Option<&'a FeatureInputTopology>>;

struct FaceInputs<'a> {
    topologies: &'a [FeatureInputTopology],
    current_faces: &'a [cadmpeg_ir::topology::Face],
    states: Option<InputStates<'a>>,
    features: Option<FeatureInputs<'a>>,
    current: Option<HashSet<&'a FaceId>>,
    members: HashMap<*const FeatureInputTopology, HashSet<&'a HistoricalFaceId>>,
}
impl<'a> FaceInputs<'a> {
    fn new(
        topologies: &'a [FeatureInputTopology],
        faces: &'a [cadmpeg_ir::topology::Face],
    ) -> Self {
        Self {
            topologies,
            current_faces: faces,
            states: None,
            features: None,
            current: None,
            members: HashMap::new(),
        }
    }
    fn state(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        state: &FeatureInputTopologyId,
    ) -> Result<Option<&'a FeatureInputTopology>, CodecError> {
        if self.states.is_none() {
            let mut states = HashMap::new();
            for topology in self.topologies {
                ctx.charge_work(
                    u64_from_index(topology.id.as_str().len())
                        + u64_from_index(topology.input_of.as_str().len())
                        + 1,
                    "index F3D emitted input states",
                )?;
                let key = (&topology.input_of, &topology.id);
                if !states.contains_key(&key) {
                    ctx.reserve_map(&mut states, 1, "index F3D emitted input states")?;
                }
                states
                    .entry(key)
                    .and_modify(|value| *value = None)
                    .or_insert(Some(topology));
            }
            self.states = Some(states);
        }
        Ok(self
            .states
            .as_ref()
            .and_then(|states| states.get(&(feature, state)))
            .copied()
            .flatten())
    }
    fn feature(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
    ) -> Result<Option<&'a FeatureInputTopology>, CodecError> {
        if self.features.is_none() {
            let mut features = HashMap::new();
            for topology in self.topologies {
                ctx.charge_work(
                    u64_from_index(topology.input_of.as_str().len()) + 1,
                    "index F3D emitted feature inputs",
                )?;
                if !features.contains_key(&topology.input_of) {
                    ctx.reserve_map(&mut features, 1, "index F3D emitted feature inputs")?;
                }
                features
                    .entry(&topology.input_of)
                    .and_modify(|value| *value = None)
                    .or_insert(Some(topology));
            }
            self.features = Some(features);
        }
        ctx.charge_work(
            u64_from_index(feature.as_str().len()) + 1,
            "query F3D emitted feature input",
        )?;
        Ok(self
            .features
            .as_ref()
            .and_then(|features| features.get(feature))
            .copied()
            .flatten())
    }
    fn members(
        &mut self,
        ctx: &DecodeContext<'_>,
        topology: &'a FeatureInputTopology,
    ) -> Result<&HashSet<&'a HistoricalFaceId>, CodecError> {
        let key = std::ptr::from_ref(topology);
        ctx.charge_work(1, "query F3D emitted topology members")?;
        if !self.members.contains_key(&key) {
            let mut faces = HashSet::new();
            for face in topology.faces.as_slice() {
                ctx.charge_work(
                    u64_from_index(face.as_str().len()) + 1,
                    "index F3D emitted historical faces",
                )?;
                ctx.insert_hash_set(&mut faces, face, "index F3D emitted historical faces")?;
            }
            ctx.insert_hash_map(
                &mut self.members,
                key,
                faces,
                "index F3D emitted topology members",
            )?;
        }
        Ok(&self.members[&key])
    }
    fn admit(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        selection: &mut FaceSelection,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        let (state, faces, native) = match selection {
            FaceSelection::Historical {
                state,
                faces,
                native,
            } => (&*state, faces.as_slice(), native.as_str()),
            FaceSelection::HistoricalPartial {
                state,
                faces,
                native,
                ..
            } => (&*state, faces.as_slice(), native.as_str()),
            _ => return Ok(()),
        };
        let admitted = self.contains(ctx, feature, state, faces)?;
        if !admitted {
            let native =
                ctx.copy_retained_text(native, "retain F3D unresolved historical selection")?;
            record_loss(ctx, feature, losses)?;
            *selection = FaceSelection::Native(native);
        }
        Ok(())
    }
    fn contains(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        state: &FeatureInputTopologyId,
        faces: &[HistoricalFaceId],
    ) -> Result<bool, CodecError> {
        ctx.charge_work(
            u64_from_index(state.as_str().len()) + u64_from_index(feature.as_str().len()),
            "admit F3D historical face selection",
        )?;
        for face in faces {
            ctx.charge_work(
                u64_from_index(face.as_str().len()) + 1,
                "query F3D emitted historical faces",
            )?;
        }
        let Some(topology) = self.state(ctx, feature, state)? else {
            return Ok(false);
        };
        if faces.is_empty() {
            return Ok(true);
        }
        let members = self.members(ctx, topology)?;
        Ok(faces.iter().all(|face| members.contains(face)))
    }

    fn profile(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        scope: Option<&str>,
        profile: &mut cadmpeg_ir::features::PlanarProfileRef,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        let cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native,
        } = profile
        else {
            return Ok(());
        };
        if !self.contains(ctx, feature, state, faces.as_slice())? {
            let source = scope
                .or_else(|| native.as_slice().first().map(String::as_str))
                .ok_or_else(|| {
                    CodecError::malformed("historical profile has no native selection")
                })?;
            let source =
                ctx.copy_retained_text(source, "retain F3D unresolved historical profile")?;
            record_loss(ctx, feature, losses)?;
            *profile = cadmpeg_ir::features::PlanarProfileRef::Native(source);
        }
        Ok(())
    }
    fn hole(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        selection: &mut FaceSelection,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if let FaceSelection::Resolved { faces, native } = selection {
            if self.current.is_none() {
                let mut current = HashSet::new();
                for face in self.current_faces {
                    ctx.charge_work(
                        u64_from_index(face.id.as_str().len()) + 1,
                        "index F3D current selection faces",
                    )?;
                    ctx.insert_hash_set(
                        &mut current,
                        &face.id,
                        "index F3D current selection faces",
                    )?;
                }
                self.current = Some(current);
            }
            let current = self.current.as_ref().ok_or_else(|| {
                CodecError::malformed("current face admission index was not built")
            })?;
            for face in faces.iter() {
                ctx.charge_work(
                    u64_from_index(face.as_str().len()) + 1,
                    "admit F3D current Hole faces",
                )?;
            }
            if !faces.iter().all(|face| current.contains(face)) {
                let native =
                    ctx.copy_retained_text(native, "retain F3D Hole selection identity")?;
                let historical = (|| -> Result<Option<FaceSelection>, CodecError> {
                    let Some(topology) = self.feature(ctx, feature)? else {
                        return Ok(None);
                    };
                    ctx.charge_work(
                        u64_from_index(topology.id.as_str().len())
                            + u64_from_index(topology.native_ref.as_deref().unwrap_or("").len())
                            + 1,
                        "query F3D Hole input topology",
                    )?;
                    let source = topology
                        .native_ref
                        .as_deref()
                        .and_then(super::historical_brep_source);
                    let Some(state) = topology
                        .id
                        .as_str()
                        .rsplit_once(':')
                        .and_then(|(_, state)| state.parse::<i64>().ok())
                    else {
                        return Ok(None);
                    };
                    if crate::ids::history_input_state_id_charged(ctx, feature, state)?
                        != topology.id
                    {
                        return Ok(None);
                    }
                    let members = self.members(ctx, topology)?;
                    let mut selected = Vec::new();
                    for face in faces.iter() {
                        ctx.charge_work(
                            u64_from_index(face.as_str().len()) + 1,
                            "resolve F3D historical Hole membership",
                        )?;
                        if !face.as_str().starts_with("f3d:brep:entity#")
                            && !source.is_some_and(|source| {
                                super::active_brep_face_matches_source(face, source)
                            })
                        {
                            return Ok(None);
                        }
                        let Some(slot) = super::stable_ref(face.as_str()) else {
                            return Ok(None);
                        };
                        let id =
                            crate::ids::history_input_face_id_charged(ctx, feature, state, slot)?;
                        if !members.contains(&id) {
                            return Ok(None);
                        }
                        ctx.push_vec(&mut selected, id, "collect F3D Hole historical faces")?;
                    }
                    let state = topology
                        .id
                        .try_clone_for_decode(ctx, "retain F3D Hole input topology")?;
                    let identity =
                        ctx.copy_retained_text(&native, "retain F3D historical Hole identity")?;
                    Ok(FaceSelection::historical(state, selected, identity, ctx)?.ok())
                })()?;
                *selection = match historical {
                    Some(value) => value,
                    None => {
                        record_loss(ctx, feature, losses)?;
                        FaceSelection::Native(native)
                    }
                };
            }
            return Ok(());
        }
        self.admit(ctx, feature, selection, losses)
    }
}
fn record_loss(
    ctx: &DecodeContext<'_>,
    feature: &FeatureId,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(
        format_args!("Feature {feature} retains its native face selection because one matching emitted input topology and its face membership could not be proved"),
        "retain F3D historical face admission loss",
    )?;
    ctx.push_vec(
        losses,
        crate::loss::F3dLossCode::HistoricalFaceSelectionUnbound.note(message),
        "collect F3D historical face admission losses",
    )
}

struct FaceAdmission<'a, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    inputs: FaceInputs<'a>,
    losses: &'ctx mut Vec<LossNote>,
}

impl FaceAdmission<'_, '_, '_> {
    fn operation(
        &mut self,
        id: &FeatureId,
        scope: Option<&str>,
        operation: &mut FeatureOperation,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let inputs = &mut self.inputs;
        let losses = &mut *self.losses;
        if let FeatureOperation::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Planar(profile),
            ..
        } = operation
        {
            inputs.profile(ctx, id, scope, profile, losses)?;
        }
        let mut requires_native = |selection: &FaceSelection| -> Result<bool, CodecError> {
            let (state, selected) = match selection {
                FaceSelection::Historical { state, faces, .. } => (state, faces.as_slice()),
                FaceSelection::HistoricalPartial { state, faces, .. } => (state, faces.as_slice()),
                _ => return Ok(false),
            };
            Ok(!inputs.contains(ctx, id, state, selected)?)
        };
        let mut edit_pair = false;
        match operation {
            FeatureOperation::FaceBlend { operands, .. } => {
                edit_pair = requires_native(operands.first_faces())?
                    || requires_native(operands.second_faces())?;
            }
            FeatureOperation::ReplaceFace { operands, .. } => {
                edit_pair = requires_native(operands.targets())?
                    || requires_native(operands.replacements())?;
            }
            FeatureOperation::FullRoundFillet { groups } => {
                for group in groups.as_slice() {
                    ctx.charge_work(1, "walk F3D full round face admission")?;
                    edit_pair |= requires_native(group.center_faces())?;
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let FullRoundSideSelection::Explicit(faces) = side {
                            edit_pair |= requires_native(faces)?;
                        }
                    }
                }
            }
            _ => {}
        }
        let mut admit = |selection: &mut FaceSelection, hole: bool| {
            if hole {
                inputs.hole(ctx, id, selection, losses)
            } else {
                inputs.admit(ctx, id, selection, losses)
            }
        };
        match operation {
            FeatureOperation::Extrude { start, extent, .. } => {
                if let ExtrudeStart::FromFace { face, .. } = start {
                    admit(face, false)?;
                }
                let mut side = |side: &mut cadmpeg_ir::features::ExtrudeSide| {
                    if let LinearTermination::ToFace { face, .. } = &mut side.termination {
                        admit(face, false)?;
                    }
                    Ok::<_, CodecError>(())
                };
                match extent {
                    ExtrudeExtent::OneSided { side: value }
                    | ExtrudeExtent::Symmetric { side: value } => side(value)?,
                    ExtrudeExtent::TwoSided { first, second } => {
                        side(first)?;
                        side(second)?;
                    }
                }
            }
            FeatureOperation::Hole {
                face: Some(face), ..
            } => admit(face, true)?,
            FeatureOperation::FullRoundFillet { groups } if edit_pair => {
                let mut admitted = Vec::new();
                for group in groups.as_slice() {
                    let mut center = group
                        .center_faces()
                        .try_clone_for_decode(ctx, "copy F3D full round face selection")?;
                    admit(&mut center, false)?;
                    let mut side = |value: &FullRoundSideSelection| -> Result<FullRoundSideSelection, CodecError> {
                        Ok(match value {
                            FullRoundSideSelection::Explicit(faces) => {
                                let mut faces = faces.try_clone_for_decode(ctx, "copy F3D full round side selection")?;
                                admit(&mut faces, false)?;
                                FullRoundSideSelection::Explicit(faces)
                            }
                            FullRoundSideSelection::Automatic => FullRoundSideSelection::Automatic,
                            FullRoundSideSelection::Unresolved => FullRoundSideSelection::Unresolved,
                        })
                    };
                    let first = side(group.side_one_faces())?;
                    let second = side(group.side_two_faces())?;
                    ctx.push_vec(
                        &mut admitted,
                        FullRoundFilletGroup::new(center, first, second, ctx)?
                            .map_err(CodecError::malformed)?,
                        "collect F3D admitted full round groups",
                    )?;
                }
                *groups = cadmpeg_ir::features::NonEmptyMembers::try_from(admitted)
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::FaceBlend { operands, .. } if edit_pair => {
                let mut first = operands
                    .first_faces()
                    .try_clone_for_decode(ctx, "copy F3D face blend selection")?;
                let mut second = operands
                    .second_faces()
                    .try_clone_for_decode(ctx, "copy F3D face blend selection")?;
                admit(&mut first, false)?;
                admit(&mut second, false)?;
                *operands = cadmpeg_ir::features::FaceBlendOperands::new(first, second, ctx)?
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::ReplaceFace { operands, .. } if edit_pair => {
                let mut first = operands
                    .targets()
                    .try_clone_for_decode(ctx, "copy F3D replace face selection")?;
                let mut second = operands
                    .replacements()
                    .try_clone_for_decode(ctx, "copy F3D replace face selection")?;
                admit(&mut first, false)?;
                admit(&mut second, false)?;
                *operands = cadmpeg_ir::features::ReplaceFaceOperands::new(first, second, ctx)?
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::CosmeticThread { face, .. } => admit(face, false)?,
            FeatureOperation::Thicken { faces, .. }
            | FeatureOperation::OffsetSurface { faces, .. }
            | FeatureOperation::KnitSurface { faces, .. }
            | FeatureOperation::TrimSurface { faces, .. }
            | FeatureOperation::ExtendSurface { faces, .. }
            | FeatureOperation::DeleteFace { faces, .. }
            | FeatureOperation::MoveFace { faces, .. }
            | FeatureOperation::Dome { faces, .. }
            | FeatureOperation::Decal { faces, .. } => admit(faces, false)?,
            FeatureOperation::FilledSurface { support_faces, .. }
            | FeatureOperation::RuledSurface { support_faces, .. } => admit(support_faces, false)?,
            FeatureOperation::SplitFace { targets, .. } => admit(targets, false)?,
            FeatureOperation::SplitBody { tools, .. }
            | FeatureOperation::CutWithSurface { tools, .. } => admit(tools, false)?,
            FeatureOperation::Shell { removed_faces, .. } => admit(removed_faces, false)?,
            FeatureOperation::MirrorShape {
                plane_reference: Some(face),
                ..
            } => admit(face, false)?,
            FeatureOperation::Pattern { seeds, .. } => {
                for seed in seeds {
                    ctx.charge_work(1, "walk F3D pattern face admission")?;
                    if let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed {
                        admit(faces, false)?;
                    }
                }
            }
            FeatureOperation::Draft { faces, anchor, .. } => {
                admit(faces, false)?;
                let (cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane: anchor, .. }
                | cadmpeg_ir::features::DraftAnchor::PartingLine { tool: anchor, .. }) = anchor;
                admit(anchor, false)?;
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn admit_feature_input_faces(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    topologies: &[FeatureInputTopology],
    faces: &[cadmpeg_ir::topology::Face],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut admission = FaceAdmission {
        ctx,
        inputs: FaceInputs::new(topologies, faces),
        losses,
    };
    for feature in features {
        ctx.charge_work(1, "walk F3D feature face admission")?;
        let id = &feature.id;
        let scope = feature.native_ref.as_deref();
        let mut result = Ok(());
        feature.evaluation.edit(|definition, _| {
            let operation = match definition {
                FeatureDefinition::Operation(operation)
                | FeatureDefinition::PostProcess { operation, .. } => operation,
            };
            result = admission.operation(id, scope, operation);
        });
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
