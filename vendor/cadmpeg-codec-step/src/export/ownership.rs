// SPDX-License-Identifier: Apache-2.0
//! Referenced construction carriers cannot become independent export roots.

use cadmpeg_core::{
    decode::{DecodeArena, DecodeContext, DecodePolicy},
    CodecError,
};
use cadmpeg_ir::{geometry::SolvedCurveGeometry, schema::EntitySchema, CadIr};
use std::collections::BTreeSet;

pub(super) fn construction_supports(ir: &CadIr) -> Result<BTreeSet<String>, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)?;
    let mut supports = BTreeSet::new();
    let mut visit = |id: &str| {
        if !supports.contains(id) {
            let id = ctx.copy_retained_text(id, "STEP export construction support identity")?;
            ctx.insert_btree_set(&mut supports, id, "STEP export construction support set")?;
        }
        Ok(())
    };
    for curve in &ir.model.curves {
        let Some(mut geometry) = curve.geometry.solved() else {
            continue;
        };
        while let SolvedCurveGeometry::Transformed(placed) = geometry {
            ctx.charge_work(1, "STEP export placed construction support")?;
            geometry = placed.basis();
        }
        if let SolvedCurveGeometry::Composite { segments, .. } = geometry {
            for segment in segments {
                ctx.charge_work(1, "STEP export composite construction support")?;
                visit(segment.curve.as_str())?;
            }
        }
    }
    for surface in &ir.model.procedural_surfaces {
        surface.visit_references(&ctx, &mut visit)?;
    }
    for curve in &ir.model.procedural_curves {
        curve.visit_references(&ctx, &mut visit)?;
    }
    for pcurve in &ir.model.pcurves {
        pcurve.visit_references(&ctx, &mut visit)?;
    }
    Ok(supports)
}

pub(super) fn is_support(
    id: &str,
    source: Option<&cadmpeg_ir::SourceObjectAssociation>,
    construction_supports: &BTreeSet<String>,
) -> bool {
    let role = source.and_then(|source| source.geometry_role);
    role == Some(cadmpeg_ir::SourceGeometryRole::Support)
        || (role != Some(cadmpeg_ir::SourceGeometryRole::Independent)
            && construction_supports.contains(id))
}
