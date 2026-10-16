// SPDX-License-Identifier: Apache-2.0
//! Numeric indexes for STEP geometry carriers.

use std::collections::HashMap;

use super::step_instance_id;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CurveIndex(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct SurfaceIndex(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PointIndex(pub(super) usize);

pub(super) struct PointCarrier {
    pub(super) index: PointIndex,
    position: Point3,
}

/// Decode-local lookup tables for STEP carrier instance ids.
pub(super) struct CarrierIndex {
    pub(super) curves: HashMap<u64, CurveIndex>,
    pub(super) points: HashMap<u64, PointCarrier>,
    pub(super) surfaces: HashMap<u64, SurfaceIndex>,
}

impl CarrierIndex {
    /// Index newly admitted surfaces without copying existing carrier maps.
    pub(super) fn refresh_surfaces(
        &mut self,
        ir: &CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        for (index, surface) in ir.model.surfaces.iter().enumerate() {
            ctx.charge_work(1, "step_carrier_surface_index")?;
            if let Some(id) = step_instance_id(surface.id.as_str()) {
                let additional = usize::from(!self.surfaces.contains_key(&id));
                ctx.reserve_map(&mut self.surfaces, additional, "step_carrier_surface_index")?;
                self.surfaces.insert(id, SurfaceIndex(index));
            }
        }
        Ok(())
    }

    pub(super) fn from_ir(ir: &CadIr, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let mut curves = HashMap::new();
        for (index, curve) in ir.model.curves.iter().enumerate() {
            if let Some(id) = step_instance_id(curve.id.as_str()) {
                ctx.reserve_map(&mut curves, 1, "step_carrier_curve_index")?;
                curves.insert(id, CurveIndex(index));
            }
        }
        let mut points = HashMap::new();
        for (index, point) in ir.model.points.iter().enumerate() {
            if let Some(id) = step_instance_id(point.id.as_str()) {
                ctx.reserve_map(&mut points, 1, "step_carrier_point_index")?;
                points.insert(
                    id,
                    PointCarrier {
                        index: PointIndex(index),
                        position: point.position().get(),
                    },
                );
            }
        }
        let mut surfaces = HashMap::new();
        for (index, surface) in ir.model.surfaces.iter().enumerate() {
            if let Some(id) = step_instance_id(surface.id.as_str()) {
                ctx.reserve_map(&mut surfaces, 1, "step_carrier_surface_index")?;
                surfaces.insert(id, SurfaceIndex(index));
            }
        }
        Ok(Self {
            curves,
            points,
            surfaces,
        })
    }

    pub(super) fn get(&self, id: u64) -> Option<&Point3> {
        self.points.get(&id).map(|point| &point.position)
    }

    pub(super) fn contains_key(&self, id: u64) -> bool {
        self.points.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{
        analytic::{LineCurve, PlaneSurface},
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, PointId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::Point;
    use cadmpeg_ir::CadIr;

    use super::CarrierIndex;

    fn carriers() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: CurveId::from(crate::ids::data(crate::ids::kind!("curve"), 1)),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("finite line"),
            )),
            source_object: None,
        });
        ir.model.points.push(Point::new(
            PointId::from(crate::ids::data(crate::ids::kind!("point"), 2)),
            cadmpeg_ir::features::FinitePoint3::ZERO,
            None,
        ));
        ir.model.surfaces.push(Surface {
            id: SurfaceId::from(crate::ids::data(crate::ids::kind!("surface"), 3)),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("finite plane"),
            )),
            source_object: None,
        });
        ir
    }

    fn assert_index_refusal(limit: u64, operation: &str) {
        let ir = carriers();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits selected policy");
        let error = CarrierIndex::from_ir(&ir, &ctx)
            .err()
            .expect("index exceeds item limit");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        ));
    }

    #[test]
    fn curve_carrier_index_refuses_collection_limit() {
        assert_index_refusal(0, "step_carrier_curve_index");
    }

    #[test]
    fn surface_extension_reuses_existing_carrier_entries() {
        let mut ir = carriers();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 4;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut index = CarrierIndex::from_ir(&ir, ctx).expect("three carrier entries");
            let mut surface = ir.model.surfaces[0].clone();
            surface.id = SurfaceId::from(crate::ids::data(crate::ids::kind!("surface"), 4));
            ir.model.surfaces.push(surface);
            index
                .refresh_surfaces(&ir, ctx)
                .expect("only the new surface allocates a slot");
            index
                .refresh_surfaces(&ir, ctx)
                .expect("existing entries allocate no slots");
            assert_eq!(index.curves[&1].0, 0);
            assert_eq!(index.points[&2].index.0, 0);
            assert_eq!(index.surfaces[&3].0, 0);
            assert_eq!(index.surfaces[&4].0, 1);
            assert_eq!(index.get(2), Some(&Point3::new(0.0, 0.0, 0.0)));
        });
    }

    #[test]
    fn point_carrier_index_refuses_collection_limit() {
        assert_index_refusal(1, "step_carrier_point_index");
    }

    #[test]
    fn surface_carrier_index_refuses_collection_limit() {
        assert_index_refusal(2, "step_carrier_surface_index");
    }
}
