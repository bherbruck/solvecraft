// SPDX-License-Identifier: Apache-2.0
//! Direct face operations: offset faces, shell, thicken, move and draft.

use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use serde::{Deserialize, Serialize};
/// Exact fixed-form construction data of a direct-face feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub(crate) enum DesignDirectFaceOperation {
    /// Signed normal offset applied to selected faces.
    OffsetFaces(DesignOffsetFacesOperation),
    /// Thin-wall shell applied after removing selected faces.
    Shell(DesignShellOperation),
    /// Signed normal thickness added from selected faces.
    Thicken(DesignThickenOperation),
}

/// Exact `OffsetFaces` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignOffsetFacesOperation {
    /// Signed distance in source centimetres.
    pub(crate) distance: FiniteReal,
    /// Referenced scalar record.
    pub(crate) distance_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) distance_offset: u64,
}

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOperation {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
    /// Whether the wall grows outward from the original boundary.
    pub(crate) outward: bool,
    /// Byte offset of the outward Boolean.
    pub(crate) outward_offset: u64,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub(crate) signed_thickness: FiniteReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
}

/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
    /// Indexed class-349 record carrying `transform`.
    pub(crate) transform_record_index: u32,
    /// Source transform-form discriminator.
    pub(crate) form: DesignMoveForm,
    /// Byte offset of `form`.
    pub(crate) form_offset: u64,
}

/// Source Move transform-form code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum DesignMoveForm {
    /// Source form 1.
    Form1,
    /// Source form 5.
    Form5,
}

impl TryFrom<u32> for DesignMoveForm {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Form1),
            5 => Ok(Self::Form5),
            _ => Err(format!("form must be 1 or 5, not {value}")),
        }
    }
}

impl From<DesignMoveForm> for u32 {
    fn from(form: DesignMoveForm) -> Self {
        match form {
            DesignMoveForm::Form1 => 1,
            DesignMoveForm::Form5 => 5,
        }
    }
}

/// Exact signed-angle lanes carried by a `Draft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub(crate) angle: Angle,
    /// Referenced draft-angle scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the draft-angle scalar.
    pub(crate) angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record.
    pub(crate) opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar.
    pub(crate) opposite_angle_offset: u64,
}

mod identity_rewrite;
