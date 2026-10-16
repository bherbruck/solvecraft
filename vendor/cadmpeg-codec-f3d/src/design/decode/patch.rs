// SPDX-License-Identifier: Apache-2.0
//! Decode the boundary-settings record a `SurfacePatch` scope references once
//! per boundary component.

use super::sketch::IndexedRecordOffsets;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::records::feature::surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Payload offset of the record's class level, past the indexed header of a
/// record whose display name is empty.
const PAYLOAD: usize = 19;

/// The boundary-settings records a `SurfacePatch` scope references, in scope
/// reference order.
///
/// The settings record occupies one fixed ordinal per boundary component in
/// each settings-bearing `SurfacePatch` scope form. Every reference member is
/// offered to the record grammar and only the members it closes are kept. The
/// single-group path form carries no settings record and therefore yields none.
pub(super) fn surface_patch_boundaries(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    reference_members: &[u32],
) -> Result<Vec<DesignSurfacePatchBoundary>, CodecError> {
    let mut boundaries = Vec::new();
    for (ordinal, record_index) in reference_members.iter().enumerate() {
        let Some(mut boundary) = records
            .first_at_or_after(0, *record_index)
            .and_then(|at| exact_surface_patch_boundary(bytes, at))
        else {
            continue;
        };
        let Ok(ordinal) = u32::try_from(ordinal) else {
            continue;
        };
        boundary.scope_reference_ordinal = ordinal;
        boundary.record_index = *record_index;

        ctx.reserve_vec(&mut boundaries, 1, "f3d SurfacePatch boundaries")?;
        boundaries.push(boundary);
    }
    Ok(boundaries)
}

/// One boundary-settings record read at the indexed header offset `at`.
///
/// The class level is two zero bytes, `u8 IsSeedSel`, `u32 PatchContinuity`,
/// `u32 PatchFlip`, `f64 PatchScale`, and the `rPatchModelRef` reference. The
/// base level's reference run closes the record and carries no settings.
fn exact_surface_patch_boundary(bytes: &[u8], at: usize) -> Option<DesignSurfacePatchBoundary> {
    let payload = at.checked_add(PAYLOAD)?;
    if View::u32_le_at(bytes, at.checked_add(15)?)? != 0
        || bytes.get(payload..payload + 2)? != [0; 2]
    {
        return None;
    }
    let is_seed_selection = match bytes.get(payload + 2)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let scale = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(bytes, payload + 11)?)?;
    let model_reference = marked_record_reference(bytes, payload + 19)?;
    Some(DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection,
        continuity: DesignPatchContinuity::from_code(View::u32_le_at(bytes, payload + 3)?),
        flip: View::u32_le_at(bytes, payload + 7)?,
        scale,
        model_reference,
    })
}
