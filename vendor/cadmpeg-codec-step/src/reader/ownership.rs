// SPDX-License-Identifier: Apache-2.0
//! Source construction roles survive failure of their neutral owners.

use super::{collect_references, source_numeric_id, ValueExt};
use crate::parse::Exchange;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{CadIr, CodecFormat, SourceGeometryRole, SourceObjectAssociation};
use std::collections::BTreeSet;

pub(super) fn mark_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "STEP source construction roles")?;
    let mut supports = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for record in exchange.records().values() {
        let definition = record
            .partials
            .iter()
            .any(|partial| partial.name == "DEFINITIONAL_REPRESENTATION");
        for partial in &record.partials {
            ctx.charge_work(1, "STEP source construction partial")?;
            if construction(&partial.name) || (definition && partial.name == "REPRESENTATION") {
                for value in &partial.parameters {
                    storage.with_storage(|| collect_references(value, &mut supports, ctx))?;
                }
            } else if independent_items(&partial.name) {
                for items in partial.parameters.iter().filter_map(ValueExt::list) {
                    for id in items.iter().filter_map(ValueExt::reference) {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(&mut roots, id, "STEP independent source geometry")
                        })?;
                    }
                }
            }
        }
    }
    for (identity, source) in ir
        .model
        .surfaces
        .iter_mut()
        .map(|surface| {
            (
                source_numeric_id(surface.id.as_str(), "surface"),
                &mut surface.source_object,
            )
        })
        .chain(ir.model.curves.iter_mut().map(|curve| {
            (
                source_numeric_id(curve.id.as_str(), "curve"),
                &mut curve.source_object,
            )
        }))
        .chain(ir.model.points.iter_mut().map(|point| {
            (
                source_numeric_id(point.id.as_str(), "point"),
                &mut point.source_object,
            )
        }))
    {
        ctx.charge_work(1, "STEP source support association")?;
        let Some(id) = identity.filter(|id| supports.contains(id)) else {
            continue;
        };
        let role = if roots.contains(&id) {
            SourceGeometryRole::Independent
        } else {
            SourceGeometryRole::Support
        };
        if source.is_none() {
            let object_id = cadmpeg_core::text::NonBlankString::new(
                ctx.format_retained(format_args!("#{id}"), "STEP source support identity")?,
            )
            .ok_or_else(|| CodecError::malformed("STEP source support identity is blank"))?;
            *source = Some(SourceObjectAssociation {
                format: CodecFormat::Step,
                geometry_role: Some(role),
                object_id,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
        } else if let Some(source) = source {
            source.geometry_role = Some(role);
        }
    }
    Ok(())
}

// EXPRESS geometry, placements and topology define their referenced geometry.
// Root item collections and presentation are separate: they do not establish
// construction ownership merely by referring to a carrier.
fn construction(name: &str) -> bool {
    matches!(
        name,
        "ADVANCED_FACE"
            | "FACE_SURFACE"
            | "EDGE_CURVE"
            | "VERTEX_POINT"
            | "EDGE"
            | "SUBEDGE"
            | "ORIENTED_EDGE"
            | "SEAM_EDGE"
            | "FACE"
            | "SUBFACE"
            | "ORIENTED_FACE"
            | "FACE_BOUND"
            | "FACE_OUTER_BOUND"
            | "EDGE_LOOP"
            | "LOOP"
            | "CONNECTED_FACE_SET"
            | "CONNECTED_FACE_SUB_SET"
            | "CLOSED_SHELL"
            | "OPEN_SHELL"
            | "ORIENTED_CLOSED_SHELL"
            | "ORIENTED_OPEN_SHELL"
            | "CONNECTED_EDGE_SET"
            | "CONNECTED_EDGE_SUB_SET"
            | "VERTEX_SHELL"
            | "WIRE_SHELL"
            | "SHELL_BASED_SURFACE_MODEL"
            | "SHELL_BASED_WIREFRAME_MODEL"
            | "FACE_BASED_SURFACE_MODEL"
            | "EDGE_BASED_WIREFRAME_MODEL"
            | "MANIFOLD_SOLID_BREP"
            | "FACETED_BREP"
            | "BREP_WITH_VOIDS"
            | "POLY_LOOP"
            | "VERTEX_LOOP"
            | "COMPOSITE_CURVE_SEGMENT"
            | "REPARAMETRISED_COMPOSITE_CURVE_SEGMENT"
            | "TRIMMED_CURVE"
            | "COMPOSITE_CURVE"
            | "CURVE_REPLICA"
            | "OFFSET_CURVE_2D"
            | "OFFSET_CURVE_3D"
            | "SURFACE_CURVE"
            | "SEAM_CURVE"
            | "INTERSECTION_CURVE"
            | "PCURVE"
            | "DEFINITIONAL_REPRESENTATION"
            | "CURVE_BOUNDED_SURFACE"
            | "RECTANGULAR_TRIMMED_SURFACE"
            | "OFFSET_SURFACE"
            | "SURFACE_REPLICA"
            | "SURFACE_OF_LINEAR_EXTRUSION"
            | "SURFACE_OF_REVOLUTION"
            | "LINE"
            | "CIRCLE"
            | "ELLIPSE"
            | "HYPERBOLA"
            | "PARABOLA"
            | "POLYLINE"
            | "B_SPLINE_CURVE"
            | "B_SPLINE_CURVE_WITH_KNOTS"
            | "UNIFORM_CURVE"
            | "QUASI_UNIFORM_CURVE"
            | "BEZIER_CURVE"
            | "RATIONAL_B_SPLINE_CURVE"
            | "PLANE"
            | "CYLINDRICAL_SURFACE"
            | "CONICAL_SURFACE"
            | "SPHERICAL_SURFACE"
            | "TOROIDAL_SURFACE"
            | "DEGENERATE_TOROIDAL_SURFACE"
            | "B_SPLINE_SURFACE"
            | "B_SPLINE_SURFACE_WITH_KNOTS"
            | "UNIFORM_SURFACE"
            | "QUASI_UNIFORM_SURFACE"
            | "BEZIER_SURFACE"
            | "RATIONAL_B_SPLINE_SURFACE"
            | "AXIS1_PLACEMENT"
            | "AXIS2_PLACEMENT_2D"
            | "AXIS2_PLACEMENT_3D"
            | "PLACEMENT"
            | "CONIC"
            | "ELEMENTARY_SURFACE"
            | "SWEPT_SURFACE"
            | "CARTESIAN_TRANSFORMATION_OPERATOR"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_2D"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_3D"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_2DNON_UNIFORM"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_3DNON_UNIFORM"
            | "VECTOR"
            | "POINT_ON_CURVE"
            | "POINT_ON_SURFACE"
    )
}

fn independent_items(name: &str) -> bool {
    matches!(
        name,
        "GEOMETRIC_SET"
            | "GEOMETRIC_CURVE_SET"
            | "REPRESENTATION"
            | "SHAPE_REPRESENTATION"
            | "SHAPE_REPRESENTATION_WITH_PARAMETERS"
            | "ADVANCED_BREP_SHAPE_REPRESENTATION"
            | "FACETED_BREP_SHAPE_REPRESENTATION"
            | "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
            | "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION"
            | "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION"
            | "EDGE_BASED_WIREFRAME_SHAPE_REPRESENTATION"
            | "SHELL_BASED_WIREFRAME_SHAPE_REPRESENTATION"
    )
}

#[cfg(test)]
mod tests;
