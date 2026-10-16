// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use crate::native::F3dNative;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Sense;

pub(super) fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

pub(super) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let decode_ctx = &decode_ctx;

    let mut projected = crate::design::configurations::project_configurations(
        decode_ctx,
        &native.design_configurations,
    )?;
    crate::design::configurations::bind_configuration_parameter_overrides(
        decode_ctx,
        &mut projected,
        &target.model.parameters,
    )?;
    crate::design::configurations::bind_configuration_suppressed_features(
        decode_ctx,
        &mut projected,
        &target.model.features,
    )?;
    if target.model.configurations != projected {
        return Err(CodecError::Malformed(
            "neutral F3D configurations must equal the projection of native configuration tables"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_assembly_projection(
    target: &CadIr,
    native: Option<&F3dNative>,
) -> Result<(), CodecError> {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let decode_ctx = &decode_ctx;

    let Some(native) = native else {
        return target
            .model
            .assembly_joints
            .is_empty()
            .then_some(())
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D generation does not support assembly joints".into(),
                )
            });
    };
    let projected = crate::design::assembly::project_assembly_joints(
        decode_ctx,
        &native.design_parameter_scopes,
        &native.design_component_occurrences,
        &target.model.features,
    )?;
    if target.model.assembly_joints != projected {
        return Err(CodecError::NotImplemented(
            "editing F3D assembly joints is not supported".into(),
        ));
    }
    Ok(())
}

pub(super) fn normalized_face_sense_to_native(desired: Sense, carrier_flipped: bool) -> Sense {
    if carrier_flipped {
        match desired {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        }
    } else {
        desired
    }
}

pub(super) fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

pub(super) fn unique_knot_count(knots: &[f64]) -> usize {
    knots
        .iter()
        .enumerate()
        .filter(|(index, value)| *index == 0 || knots[*index - 1] != **value)
        .count()
}
