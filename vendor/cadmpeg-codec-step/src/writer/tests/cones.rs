// SPDX-License-Identifier: Apache-2.0
//! STEP writer cone chart tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn negative_cone_face_pcurve_is_written_in_reversed_chart() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    let surface_id = ir.model.faces[0].surface.clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .expect("face surface");
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("negative cone"),
    ));

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write cone pcurve");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PCURVE("));
    assert!(text.contains("CURVE_REPLICA("));
    assert!(text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_2D("));
    assert!(text.contains("DIRECTION('',(-1.,0.))"));
    assert!(text.contains("DIRECTION('',(0.,-1.))"));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PcurveCarrierUnwritable.kind()));
}

#[test]
fn unmappable_negative_cone_pcurve_loss_names_its_face() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    let face_id = ir.model.faces[0].id.clone();
    let surface_id = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .expect("face surface")
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("negative cone"),
    ));
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Harmonic(
        cadmpeg_ir::geometry::pcurve::HarmonicPcurve::try_new(
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
            cadmpeg_ir::math::Point2::new(0.0, 1.0),
        )
        .expect("harmonic pcurve"),
    );

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write cone without unmappable pcurve");
    assert!(!String::from_utf8(output)
        .expect("STEP output is UTF-8")
        .contains("PCURVE("));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.message.contains(face_id.as_str())
            && loss.message.contains(ir.model.pcurves[0].id.as_str())
    }));
}
