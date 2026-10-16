// SPDX-License-Identifier: Apache-2.0
//! Literal admission for presentation attributes that cannot define shape.

use super::Value;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// These EXPRESS presentation families describe styling and layer assignment.
/// Placements, coordinates, geometry contexts, units and shape records keep
/// required literal admission even when a presentation graph references them.
pub(super) fn presentation_record(name: &str) -> bool {
    matches!(
        name,
        "COLOUR"
            | "COLOUR_SPECIFICATION"
            | "PRE_DEFINED_COLOUR"
            | "BACKGROUND_COLOUR"
            | "SYMBOL_COLOUR"
            | "COLOUR_RGB"
            | "COLOUR_RGB_LIST"
            | "DRAUGHTING_PRE_DEFINED_COLOUR"
            | "CURVE_STYLE"
            | "CURVE_STYLE_FONT"
            | "CURVE_STYLE_FONT_PATTERN"
            | "PRE_DEFINED_CURVE_FONT"
            | "DRAUGHTING_PRE_DEFINED_CURVE_FONT"
            | "EXTERNALLY_DEFINED_CURVE_FONT"
            | "PRE_DEFINED_TEXT_FONT"
            | "EXTERNALLY_DEFINED_TEXT_FONT"
            | "POINT_STYLE"
            | "FILL_AREA_STYLE"
            | "FILL_AREA_STYLE_COLOUR"
            | "FILL_AREA_STYLE_HATCHING"
            | "FILL_AREA_STYLE_TILE_SYMBOL_WITH_STYLE"
            | "FILL_AREA_STYLE_TILES"
            | "EXTERNALLY_DEFINED_HATCH_STYLE"
            | "EXTERNALLY_DEFINED_TILE_STYLE"
            | "SYMBOL_STYLE"
            | "SURFACE_STYLE_USAGE"
            | "SURFACE_SIDE_STYLE"
            | "SURFACE_STYLE_FILL_AREA"
            | "SURFACE_STYLE_BOUNDARY"
            | "SURFACE_STYLE_SILHOUETTE"
            | "SURFACE_STYLE_CONTROL_GRID"
            | "SURFACE_STYLE_SEGMENTATION_CURVE"
            | "SURFACE_STYLE_PARAMETER_LINE"
            | "SURFACE_STYLE_RENDERING"
            | "SURFACE_STYLE_RENDERING_WITH_PROPERTIES"
            | "SURFACE_STYLE_TRANSPARENT"
            | "SURFACE_STYLE_REFLECTANCE_AMBIENT"
            | "SURFACE_STYLE_REFLECTANCE_AMBIENT_DIFFUSE"
            | "SURFACE_STYLE_REFLECTANCE_AMBIENT_DIFFUSE_SPECULAR"
            | "TEXT_STYLE"
            | "TEXT_STYLE_FOR_DEFINED_FONT"
            | "TEXT_STYLE_WITH_BOX_CHARACTERISTICS"
            | "TEXT_STYLE_WITH_MIRROR"
            | "PRESENTATION_STYLE_ASSIGNMENT"
            | "PRESENTATION_STYLE_BY_CONTEXT"
            | "PRESENTATION_LAYER_ASSIGNMENT"
            | "PRESENTATION_LAYER_USAGE"
            | "STYLED_ITEM"
            | "OVER_RIDING_STYLED_ITEM"
            | "CONTEXT_DEPENDENT_OVER_RIDING_STYLED_ITEM"
    )
}

pub(super) fn unreadable_literal(
    values: &[Value],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let _depth = ctx.enter_nested("STEP metadata literal walk")?;
    for value in values {
        ctx.charge_work(1, "STEP metadata literal walk")?;
        let unreadable = match value {
            Value::UninterpretedLiteral => true,
            Value::List(values) => unreadable_literal(values, ctx)?,
            Value::Typed(_, value) => {
                unreadable_literal(std::slice::from_ref(value.as_ref()), ctx)?
            }
            _ => false,
        };
        if unreadable {
            return Ok(true);
        }
    }
    Ok(false)
}
