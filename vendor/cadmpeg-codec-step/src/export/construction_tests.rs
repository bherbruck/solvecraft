// SPDX-License-Identifier: Apache-2.0
//! Supported constructions remain writable without a solved surface cache.

use cadmpeg_ir::{
    codec::{Codec, DecodeOptions},
    document::admission::StandardAdmission,
    geometry::{
        analytic::LineCurve, surface_payloads::LinearSweepSurfaceConstruction, Curve,
        CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
        SolvedSurfaceGeometry, SurfaceGeometry,
    },
    ids::{CurveId, ProceduralSurfaceId},
};
use std::io::Cursor;

#[test]
fn a_writable_surface_construction_preserves_faces_without_a_solved_cache() {
    for opaque_cache in [false, true] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("cube plane")
        };
        let directrix = CurveId::mint("test:model:curve#directrix").unwrap();
        let direction = plane.frame().binormal().into();
        ir.model.curves.push(Curve {
            id: directrix.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(plane.origin().get(), *plane.frame().reference().as_raw())
                    .unwrap(),
            )),
            parameter_range: None,
            source_object: None,
        });
        let owner = ir.model.surfaces[0].id.clone();
        ir.model
            .add_procedural_surface(
                &StandardAdmission,
                &owner,
                ProceduralSurface::new(
                    ProceduralSurfaceId::mint("test:model:procedural-surface#sweep").unwrap(),
                    ProceduralSurfaceDefinition::LinearSweep(
                        LinearSweepSurfaceConstruction::try_new(directrix, direction).unwrap(),
                    ),
                    None,
                ),
            )
            .unwrap()
            .unwrap();
        let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[0].geometry else {
            panic!("construction owner")
        };
        *cache = opaque_cache.then_some(SolvedSurfaceGeometry::Unknown { record: None });
        let checked = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
        assert!(checked.is_ok(), "{:?}", checked.findings);
        let mut bytes = Vec::new();
        let report = super::write_step(
            &ir,
            &mut bytes,
            crate::StepSchema::Ap214,
            &crate::StepWriteOptions::default(),
        )
        .unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text.matches("ADVANCED_FACE").count(), 6);
        assert!(text.contains("SURFACE_OF_LINEAR_EXTRUSION"));
        assert!(!report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::UnknownSurfaceFaceOmitted.kind()));
        let decoded = crate::StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.faces.len(), 6);
        assert_eq!(decoded.ir().model.bodies.len(), 1);
        let checked = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).unwrap();
        assert!(checked.is_ok(), "{:?}", checked.findings);
    }
}
