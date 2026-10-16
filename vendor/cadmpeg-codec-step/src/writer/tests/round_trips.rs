// SPDX-License-Identifier: Apache-2.0
//! STEP writer round-trip and emission tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::PcurveGeometry,
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::test_support::exchange::export;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn curve_geometry_for_sheet_pcurve(
    geometry: &PcurveGeometry,
) -> Result<Option<CurveGeometry>, cadmpeg_ir::geometry::nurbs::NurbsError> {
    let point = |point: Point2| Point3::new(point.u, point.v, 0.0);
    let vector = |vector: Point2| Vector3::new(vector.u, vector.v, 0.0);
    let line = |origin: Point2, direction: Point2| {
        let length = direction.u.hypot(direction.v);
        (length.is_finite() && length > 0.0).then(|| {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    point(origin),
                    vector(Point2::new(direction.u / length, direction.v / length)),
                )
                .unwrap(),
            ))
        })
    };
    Ok(match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            line(*origin, *direction)
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let radius = circle_pcurve.radius();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    point(center.get()),
                    Vector3::new(0.0, 0.0, 1.0),
                    vector(x_axis.get()),
                    radius.get(),
                )
                .unwrap(),
            )))
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                    point(center.get()),
                    Vector3::new(0.0, 0.0, 1.0),
                    vector(x_axis.get()),
                    major_radius.get(),
                    minor_radius.get(),
                )
                .unwrap(),
            )))
        }
        PcurveGeometry::Parabola(parabola_pcurve) => {
            let vertex = parabola_pcurve.vertex();
            let x_axis = parabola_pcurve.x_axis();
            let focal_distance = parabola_pcurve.focal_distance();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                    point(vertex.get()),
                    Vector3::new(0.0, 0.0, 1.0),
                    vector(x_axis.get()),
                    focal_distance.get(),
                )
                .unwrap(),
            )))
        }
        PcurveGeometry::Hyperbola(hyperbola_pcurve) => {
            let center = hyperbola_pcurve.center();
            let x_axis = hyperbola_pcurve.x_axis();
            let major_radius = hyperbola_pcurve.major_radius();
            let minor_radius = hyperbola_pcurve.minor_radius();
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
                    point(center.get()),
                    Vector3::new(0.0, 0.0, 1.0),
                    vector(x_axis.get()),
                    major_radius.get(),
                    minor_radius.get(),
                )
                .unwrap(),
            )))
        }
        PcurveGeometry::Nurbs { nurbs } => Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                nurbs.degree(),
                nurbs.knots().to_vec(),
                nurbs
                    .pole_rows()
                    .raw_points()
                    .into_iter()
                    .map(point)
                    .collect(),
                nurbs.pole_rows().weights(),
                nurbs.periodic(),
            )
            .expect("fixture constructor admission")?,
        ))),
        PcurveGeometry::Transformed(placed) => {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))) =
                curve_geometry_for_sheet_pcurve(placed.basis())?
            else {
                return Ok(None);
            };
            let transform = placed.transform();
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            let transform = Transform::affine([
                [
                    transform.rows()[0][0],
                    transform.rows()[0][1],
                    0.0,
                    transform.rows()[0][2],
                ],
                [
                    transform.rows()[1][0],
                    transform.rows()[1][1],
                    0.0,
                    transform.rows()[1][2],
                ],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform");
            let (Some(direction), Some(placed_origin)) = (
                transform.apply_vector(direction),
                transform.apply_point(origin),
            ) else {
                return Ok(None);
            };
            let length = direction.norm();
            (length.is_finite() && length > 0.0).then(|| {
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        placed_origin.get(),
                        direction.scale(1.0 / length),
                    )
                    .unwrap(),
                ))
            })
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let basis = trimmed_pcurve.basis();
            curve_geometry_for_sheet_pcurve(basis)?
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let Some((origin, direction)) = basis
                .line_parameters(&cadmpeg_test_support::service_decode_context())
                .expect("fixture line parameter walk is admitted")
            else {
                return Ok(None);
            };
            let length = direction.u.hypot(direction.v);
            if !length.is_finite() || length == 0.0 {
                return Ok(None);
            }
            line(
                Point2::new(
                    origin.u - distance.get() * direction.v / length,
                    origin.v + distance.get() * direction.u / length,
                ),
                Point2::new(direction.u / length, direction.v / length),
            )
        }
        PcurveGeometry::PolarHarmonic(_) => None,
        PcurveGeometry::PolarNurbs { .. } => None,
        PcurveGeometry::SphericalGreatCircle(_) => None,
        PcurveGeometry::Harmonic(_) => None,
        PcurveGeometry::Hyperbolic(_) => None,
    })
}

fn align_sheet_edge_to_pcurve(
    ir: &mut CadIr,
    geometry: &PcurveGeometry,
) -> Result<(), cadmpeg_ir::geometry::nurbs::NurbsError> {
    let pcurve_id = ir.model.pcurves[0].id.clone();
    let (curve_id, point_ids) = {
        let edge_id = ir
            .model
            .coedges
            .iter()
            .find(|coedge| {
                coedge
                    .pcurves
                    .iter()
                    .any(|pcurve| pcurve.pcurve == pcurve_id)
            })
            .expect("sheet pcurve coedge")
            .edge
            .clone();
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == edge_id)
            .expect("sheet pcurve edge");
        (
            edge.curve().cloned().expect("sheet pcurve edge curve"),
            [edge.start.clone(), edge.end.clone()],
        )
    };
    if let Some(curve_geometry) = curve_geometry_for_sheet_pcurve(geometry)? {
        ir.model
            .curves
            .iter_mut()
            .find(|curve| curve.id == curve_id)
            .expect("sheet pcurve curve")
            .geometry = curve_geometry;
    }
    let point_ids = point_ids.map(|vertex_id| {
        ir.model
            .vertices
            .iter()
            .find(|vertex| vertex.id == vertex_id)
            .expect("sheet edge vertex")
            .point
            .clone()
    });
    let parameter_range = match geometry {
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            parameter_range.endpoints()
        }
        _ => [0.0, 1.0],
    };
    let positions = parameter_range.map(|parameter| {
        let uv = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            geometry,
            parameter,
        )
        .expect("test pcurve endpoint");
        Point3::new(uv.u, uv.v, 0.0)
    });
    for (point_id, position) in point_ids.into_iter().zip(positions) {
        ir.model
            .points
            .iter_mut()
            .find(|point| point.id == point_id)
            .expect("sheet edge point")
            .set_position(
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
            );
    }
    Ok(())
}

/// Emit a single surface carrier in isolation and return the DATA lines joined.
fn emit_surface_only(g: &SurfaceGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::surface(&mut e, g.solved().expect("solved surface"))
        .expect("surface geometry is writable");
    e.into_lines().expect("finite reals").join("\n")
}

#[test]
fn negative_cone_writes_reversed_axis_positive_angle_and_exact_points() {
    const EPS_CONE_POINT: f64 = 1.0e-9;
    let origin = Point3::new(1.0, 2.0, 3.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let source = SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            origin, axis, reference, 2.0, 1.0, -0.5,
        )
        .expect("finite cone"),
    );
    let text = emit_surface_only(&SurfaceGeometry::Solved(source.clone()));
    assert!(text.contains("CARTESIAN_POINT('',(1.,2.,3.))"));
    assert!(text.contains("DIRECTION('',(-0.,-0.,-1.))"));
    assert!(text.contains("DIRECTION('',(1.,0.,0.))"));
    assert!(text.contains("CONICAL_SURFACE('',#4,2.,0.5)"));

    let written = SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            origin,
            Vector3::new(-axis.x, -axis.y, -axis.z),
            reference,
            2.0,
            1.0,
            0.5,
        )
        .expect("written STEP cone"),
    );
    for (u, v) in [(0.0, 0.0), (0.3, 0.7), (-1.2, 1.1), (2.5, -0.4)] {
        let source_point = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &source,
            u,
            v,
        )
        .expect("source point")
        .get();
        let written_point = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &written,
            -u,
            -v,
        )
        .expect("written point")
        .get();
        let distance = (source_point.x - written_point.x)
            .hypot(source_point.y - written_point.y)
            .hypot(source_point.z - written_point.z);
        assert!(distance < EPS_CONE_POINT);
    }
}

#[test]
fn negative_cone_round_trip_preserves_point_set_and_face_sense() {
    const EPS_CONE_ROUND_TRIP_POINT: f64 = 1.0e-9;
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let source = SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("finite cone"),
    );
    let source_surface = ir.model.surfaces[0].id.clone();
    let source_face_sense = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == source_surface)
        .expect("cone face")
        .sense;
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(source.clone());

    let output = export(&ir);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written cone");
    let (surface_id, written) = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Cone(cone)) => {
                Some((&surface.id, SolvedSurfaceGeometry::Cone(*cone)))
            }
            _ => None,
        })
        .expect("written cone surface");
    let written_face = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| &face.surface == surface_id)
        .expect("written cone face");
    assert_eq!(written_face.sense, source_face_sense);
    for (u, v) in [(0.0, 0.0), (0.3, 0.7), (-1.2, 1.1), (2.5, -0.4)] {
        let source_point = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &source,
            u,
            v,
        )
        .expect("source point")
        .get();
        let written_point = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &written,
            -u,
            -v,
        )
        .expect("written point")
        .get();
        let distance = (source_point.x - written_point.x)
            .hypot(source_point.y - written_point.y)
            .hypot(source_point.z - written_point.z);
        assert!(distance < EPS_CONE_ROUND_TRIP_POINT);
    }
}

#[test]
fn negative_cone_rectangular_trim_maps_ranges_and_senses() {
    let source = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,2.,-0.5);
#6=RECTANGULAR_TRIMMED_SURFACE('',#5,0.25,1.25,2.,3.,.T.,.F.);
#7=GEOMETRIC_SET('',(#6));
#8=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#7),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";
    let decoded = crate::test_support::exchange::decode_inline(source);
    let output = export(decoded.ir());
    assert!(output.contains("CONICAL_SURFACE("));
    assert!(output.contains("RECTANGULAR_TRIMMED_SURFACE("));
    assert!(output.contains(",-0.25,-1.25,-2.,-3.,.F.,.T.)"));
}

#[test]
fn negative_cone_nested_trim_keeps_outer_local_ranges() {
    let source = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,2.,-0.5);
#6=RECTANGULAR_TRIMMED_SURFACE('',#5,0.25,1.25,2.,3.,.T.,.F.);
#7=RECTANGULAR_TRIMMED_SURFACE('',#6,0.1,0.2,0.5,1.,.T.,.T.);
#8=GEOMETRIC_SET('',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";
    let decoded = crate::test_support::exchange::decode_inline(source);
    let output = export(decoded.ir());
    assert!(output.contains(",-0.25,-1.25,-2.,-3.,.F.,.T.)"));
    assert!(output.contains(",0.1,0.2,0.5,1.,.T.,.T.)"));
}

/// Emit a single curve carrier in isolation and return the DATA lines joined.
fn emit_curve_only(g: &CurveGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::curve(&mut e, g.solved().expect("solved curve"))
        .expect("curve geometry is writable");
    e.into_lines().expect("finite reals").join("\n")
}

fn buf_line_count(buf: &[u8]) -> usize {
    // Count DATA-section instance lines: those starting with '#'.
    String::from_utf8_lossy(buf)
        .lines()
        .filter(|l| l.starts_with('#'))
        .count()
}

/// A minimal single-cylinder-surface document exercising analytic emission and
/// interning of shared points/directions.
pub(super) fn cylinder_surface_doc() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:surface#cyl").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir
}

#[test]
pub(crate) fn writer_round_trips_rational_nurbs_pcurves(
) -> Result<(), cadmpeg_ir::geometry::nurbs::NurbsError> {
    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    let mut ir = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(10.0, 0.0),
            ],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    };
    let geometry = ir.model.pcurves[0].geometry.clone();
    align_sheet_edge_to_pcurve(&mut ir, &geometry)?;

    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write NURBS pcurve");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode NURBS pcurve");
    assert!(matches!(
        &decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs }
            if nurbs.degree() == 1
                && !nurbs.periodic()
                && nurbs.control_points().len() == 2
                && nurbs.pole_rows().weights() == Some(vec![1.0, 2.0])
    ));
    Ok(())
}

#[test]
fn writer_round_trips_every_exact_step_pcurve_family(
) -> Result<(), cadmpeg_ir::geometry::nurbs::NurbsError> {
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::transform::Transform2;

    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    let template = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .into_parts()
        .0;
    let x_axis = Point2::new(0.6, 0.8);
    let y_axis = Point2::new(-0.8, 0.6);
    let cases = [
        PcurveGeometry::Circle(
            cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
                Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                4.0,
            )
            .unwrap(),
        ),
        PcurveGeometry::Ellipse(
            cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                4.0,
                2.0,
            )
            .unwrap(),
        ),
        PcurveGeometry::Parabola(
            cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
                Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                1.5,
            )
            .unwrap(),
        ),
        PcurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::try_new(
                Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                4.0,
                2.0,
            )
            .unwrap(),
        ),
        PcurveGeometry::Trimmed(
            cadmpeg_ir::geometry::pcurve::TrimmedPcurve::try_new(
                [0.25, 1.75],
                true,
                Box::new(PcurveGeometry::Circle(
                    cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
                        Point2::new(2.0, 3.0),
                        x_axis,
                        y_axis,
                        4.0,
                    )
                    .unwrap(),
                )),
            )
            .unwrap(),
        ),
        PcurveGeometry::Offset(
            cadmpeg_ir::geometry::pcurve::OffsetPcurve::try_new(
                -0.5,
                Box::new(PcurveGeometry::Line(
                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(2.0, 3.0),
                        Point2::new(4.0, 0.0),
                    )
                    .unwrap(),
                )),
            )
            .unwrap(),
        ),
        PcurveGeometry::Transformed(
            cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
                Box::new(PcurveGeometry::Line(
                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(1.0, 2.0),
                        Point2::new(3.0, 4.0),
                    )
                    .unwrap(),
                )),
                Transform2::affine([[0.0, -2.0, 10.0], [2.0, 0.0, 20.0]])
                    .expect("affine transform"),
            )
            .expect("placed pcurve"),
        ),
    ];

    for geometry in cases {
        let mut ir = template.clone();
        ir.model.pcurves[0].geometry = geometry.clone();
        align_sheet_edge_to_pcurve(&mut ir, &geometry)?;
        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("write exact pcurve");
        let output_text = String::from_utf8(output).expect("STEP output is UTF-8");
        if matches!(&geometry, PcurveGeometry::Transformed(_)) {
            assert!(output_text.contains("CURVE_REPLICA"));
            assert!(output_text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_2D"));
        }
        let decoded = StepCodec::default()
            .decode(
                &mut Cursor::new(output_text.into_bytes()),
                &DecodeOptions::default(),
            )
            .expect("decode exact pcurve");
        assert_eq!(decoded.ir().model.pcurves[0].geometry, geometry);
        assert_eq!(decoded.ir().model.bodies.len(), 1);
        assert!(decoded
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("has no decoded surface or 2D curve")));
    }
    Ok(())
}

#[test]
pub(crate) fn writer_round_trips_rigid_body_placements() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
        ])
        .expect("affine transform"),
    );
    let options = StepWriteOptions {
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, StepSchema::Ap214, &options).expect("write placed body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode placed body");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].transform,
        ir.model.bodies[0].transform
    );
}

#[test]
pub(crate) fn writer_round_trips_product_body_ownership() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let product =
        cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:product-definition#product-0")
            .expect("identity grammar");
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Cube part".into()),
            label: Some("Cube part".into()),
            description: None,
            part_number: Some("PART-001".into()),
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId::mint("test:model:occurrence#root-0")
            .expect("identity grammar"),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root {},
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: Some("Cube root".into()),
        visible: None,
        link: None,
        native_ref: None,
    });
    let options = StepWriteOptions {
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write product-owned body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode product-owned body");
    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0]
            .part_number
            .as_deref(),
        Some("PART-001")
    );
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
}

fn wire_body_ir(alpha: f32) -> CadIr {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let edge = ir.model.edges[0].clone();
    let curve = edge.curve().cloned().expect("cube edge curve");
    ir.model.edges.retain(|candidate| candidate.id == edge.id);
    ir.model.curves.retain(|candidate| candidate.id == curve);
    ir.model
        .vertices
        .retain(|vertex| vertex.id == edge.start || vertex.id == edge.end);
    let point_ids = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<Vec<_>>();
    ir.model
        .points
        .retain(|point| point_ids.contains(&point.id));
    ir.model.coedges.clear();
    ir.model.loops.clear();
    ir.model.faces.clear();
    ir.model.surfaces.clear();
    ir.model.shells.truncate(1);
    ir.model.shells[0]
        .edit_topology(|faces, wire_edges, free_vertices| {
            faces.clear();
            *wire_edges = vec![edge.id];
            free_vertices.clear();
        })
        .unwrap();
    ir.model.regions.truncate(1);
    ir.model.regions[0].shells = vec![ir.model.shells[0].id.clone()];
    ir.model.bodies.truncate(1);
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.bodies[0].color =
        Some(cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.8, alpha).expect("valid color"));
    ir.model.bodies[0].regions = vec![ir.model.regions[0].id.clone()];
    ir
}

#[test]
pub(crate) fn writer_round_trips_edge_based_wire_bodies() {
    let ir = wire_body_ir(1.0);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write wire body");
    let text = String::from_utf8(output.clone()).expect("wire STEP is UTF-8");
    assert!(text.contains("CURVE_STYLE"));
    assert_eq!(text.matches("STYLED_ITEM").count(), 1);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode wire body");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(decoded.ir().model.edges.len(), 1);
    assert_eq!(decoded.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].color,
        Some(cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.8, 1.0).expect("valid color"))
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn translucent_direct_wire_body_color_reports_curve_style_alpha_loss() {
    let ir = wire_body_ir(0.5);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write translucent wire body");
    let text = String::from_utf8(output).expect("wire STEP is UTF-8");
    assert!(text.contains("CURVE_STYLE"));
    assert!(!text.contains("SURFACE_STYLE_TRANSPARENT"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireBodyTransparencyOmitted.kind()
            && loss.message.contains("wire body")
            && loss.message.contains("transparency")
    }));
}

#[test]
fn writer_round_trips_standalone_points_and_curves() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.curves.truncate(1);
    ir.model.surfaces.clear();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();

    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write standalone geometry");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode standalone geometry");
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert_eq!(decoded.ir().model.points.len(), ir.model.points.len());
    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
pub(crate) fn ap242_writer_round_trips_indexed_tessellation_and_exact_body_link() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#mesh-0")
                .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                vec![[0, 1, 2], [2, 1, 0]],
                Some(vec![Vector3::new(0.0, 0.0, 1.0); 3]),
            )
            .expect("normals cover the vertices"),
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_body(Some(ir.model.bodies[0].id.clone())),
    );
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write AP242 tessellation");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("tessellation")));
    let text = String::from_utf8(bytes.clone()).expect("STEP text");
    assert_eq!(text.matches("TRIANGULATED_FACE(").count(), 1);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    let mesh = &decoded.ir().model.tessellations[0];
    assert_eq!(mesh.vertices().len(), 3);
    assert_eq!(mesh.triangles(), [[0, 1, 2], [2, 1, 0]]);
    assert_eq!(mesh.vertex_normals().len(), 3);
    assert!(mesh.body.is_some());
}

#[test]
pub(crate) fn analytic_conics_round_trip_through_step() {
    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            2.5,
        )
        .unwrap(),
    ));
    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            4.0,
            1.5,
        )
        .unwrap(),
    ));
    let mut source = CadIr::empty();
    source.model.curves.extend([
        Curve {
            parameter_range: None,
            id: CurveId::mint("test:model:curve#parabola").expect("identity grammar"),
            geometry: parabola.clone(),
            source_object: None,
        },
        Curve {
            parameter_range: None,
            id: CurveId::mint("test:model:curve#hyperbola").expect("identity grammar"),
            geometry: hyperbola.clone(),
            source_object: None,
        },
    ]);

    let mut output = Vec::new();
    write_step(
        &source,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write conics");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode conics");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == parabola));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == hyperbola));
}

#[test]
pub(crate) fn standalone_geometry_uses_general_shape_representation() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("test:model:curve#line").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let output = export(&ir);
    assert!(output.contains("SHAPE_REPRESENTATION('',"));
    assert!(!output.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
}

#[test]
fn cube_has_valid_part21_envelope() {
    let s = export(&unit_cube().expect("unit cube fixture is admitted"));
    assert!(s.starts_with("ISO-10303-21;\n"));
    assert!(s.contains("HEADER;"));
    assert!(s.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));"));
    assert!(s.contains("\nDATA;\n"));
    assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
    // ENDSEC appears twice: once closing HEADER, once closing DATA.
    assert_eq!(s.matches("ENDSEC;").count(), 2);
}

#[test]
fn cube_emits_full_brep_hierarchy() {
    let s = export(&unit_cube().expect("unit cube fixture is admitted"));
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("CLOSED_SHELL"));
    // Six planar faces, twelve unique edges, eight vertices.
    assert_eq!(s.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(s.matches("= PLANE(").count(), 6);
    assert_eq!(s.matches("EDGE_CURVE").count(), 12);
    assert_eq!(s.matches("VERTEX_POINT").count(), 8);
    // 6 loops * 4 coedges = 24 oriented edges.
    assert_eq!(s.matches("ORIENTED_EDGE").count(), 24);
    assert_eq!(s.matches("= EDGE_LOOP(").count(), 6);
    assert_eq!(s.matches("FACE_OUTER_BOUND").count(), 0);
    assert_eq!(s.matches("FACE_BOUND").count(), 6);
    // Every line edge carries a LINE curve.
    assert_eq!(s.matches("= LINE(").count(), 12);
}

#[test]
fn cube_product_and_context_boilerplate_present() {
    let s = export(&unit_cube().expect("unit cube fixture is admitted"));
    for kw in [
        "APPLICATION_CONTEXT",
        "APPLICATION_PROTOCOL_DEFINITION",
        "PRODUCT(",
        "PRODUCT_DEFINITION(",
        "PRODUCT_DEFINITION_SHAPE",
        "SHAPE_DEFINITION_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "GEOMETRIC_REPRESENTATION_CONTEXT",
        "UNCERTAINTY_MEASURE_WITH_UNIT",
    ] {
        assert!(s.contains(kw), "missing {kw}");
    }
    // mm document → millimetre SI length unit.
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
}

#[test]
fn every_reference_resolves() {
    // Collect declared instance ids (#n = ...) and every #n referenced anywhere;
    // a valid Part 21 graph references only declared instances.
    let s = export(&unit_cube().expect("unit cube fixture is admitted"));
    let mut declared = std::collections::HashSet::new();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(eq) = rest.find(" =") {
                if let Ok(id) = rest[..eq].parse::<u64>() {
                    declared.insert(id);
                }
            }
        }
    }
    assert!(!declared.is_empty());
    // Scan referenced ids: '#' followed by digits, but skip the leading id of a
    // declaration line (handled by only scanning after the first '=').
    for line in s.lines() {
        let Some(eq) = line.find('=') else { continue };
        let body = &line[eq + 1..];
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start {
                    let id: u64 = body[start..j].parse().unwrap();
                    assert!(
                        declared.contains(&id),
                        "dangling reference #{id} in: {line}"
                    );
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

#[test]
fn unclassified_face_loops_round_trip_without_inventing_outer_roles() {
    let mut ir = unit_cube().expect("unit cube fixture");
    for face in &mut ir.model.faces {
        face.loops = cadmpeg_ir::topology::FaceLoops::unspecified(face.loops.to_vec());
    }
    let output = export(&ir);
    assert_eq!(output.matches("FACE_OUTER_BOUND").count(), 0);
    assert_eq!(output.matches("FACE_BOUND").count(), 6);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
    assert!(decoded.ir().model.faces.iter().all(|face| matches!(
        face.loops,
        cadmpeg_ir::topology::FaceLoops::Unspecified { .. }
    )));
}

#[test]
fn periodic_two_rim_face_keeps_unclassified_bounds_through_export() {
    let original = include_str!("data/periodic_two_rims.p21");
    for source in [
        original.to_owned(),
        original.replace("FACE_BOUND", "FACE_OUTER_BOUND"),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.faces.len(), 1);
        assert_eq!(decoded.ir().model.loops.len(), 2);
        let output = export(decoded.ir());
        assert_eq!(output.matches("FACE_OUTER_BOUND").count(), 0);
        assert_eq!(output.matches("FACE_BOUND").count(), 2);
        let round_trip = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert_eq!(round_trip.ir().model.faces.len(), 1);
        assert_eq!(round_trip.ir().model.loops.len(), 2);
        assert!(matches!(
            round_trip.ir().model.faces[0].loops,
            cadmpeg_ir::topology::FaceLoops::Unspecified { .. }
        ));
    }
}

#[test]
fn reports_entity_counts_and_no_geometry_loss_for_cube() {
    let mut buf = Vec::new();
    let report = write_step(
        &unit_cube().expect("unit cube fixture is admitted"),
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert_eq!(report.census.total(), buf_line_count(&buf));
    assert_eq!(report.census.counts.get("ADVANCED_FACE"), Some(&6));
    assert_eq!(report.census.counts.get("VERTEX_POINT"), Some(&8));
    // The cube is fully representable: no error/blocking losses.
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.severity >= cadmpeg_ir::report::Severity::Error)
            .count(),
        0
    );
}

#[test]
fn writer_round_trips_binding_scoped_appearance_visibility() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let appearance = AppearanceId::mint("test:model:appearance#hidden").expect("identity grammar");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: Some("hidden face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.8, 0.2, 0.1, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#hidden-face"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write hidden appearance binding");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("INVISIBILITY"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode hidden appearance binding");
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| binding.visible == Some(false)));
}

#[test]
fn writer_round_trips_surface_appearance_transparency() {
    const EPS_ALPHA: f32 = 0.000_001;

    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let appearance =
        AppearanceId::mint("test:model:appearance#transparent").expect("identity grammar");
    let second_appearance =
        AppearanceId::mint("test:model:appearance#more-transparent").expect("identity grammar");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: Some("transparent face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.8, 0.2, 0.1, 0.35).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearances.push(Appearance {
        id: second_appearance.clone(),
        name: Some("more transparent face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.8, 0.2, 0.1, 0.65).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#transparent-face"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#more-transparent-face"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Face(ir.model.faces[1].id.clone()),
        appearance: second_appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write transparent surface appearance");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("SURFACE_STYLE_TRANSPARENT"));
    assert!(text.contains("SURFACE_STYLE_RENDERING_WITH_PROPERTIES"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode transparent surface appearance");
    let alphas = decoded
        .ir()
        .model
        .appearances
        .iter()
        .filter_map(|appearance| appearance.base_color.map(cadmpeg_ir::topology::Color::a))
        .collect::<Vec<_>>();
    assert_eq!(alphas.len(), 2);
    assert!(alphas.iter().any(|alpha| (*alpha - 0.35).abs() < EPS_ALPHA));
    assert!(alphas.iter().any(|alpha| (*alpha - 0.65).abs() < EPS_ALPHA));
}

#[test]
fn writer_round_trips_presentation_layer_visibility() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#hidden").expect("identity grammar"),
        name: "hidden layer".into(),
        description: Some("layer visibility".into()),
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write hidden presentation layer");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('hidden layer','layer visibility',"));
    assert!(text.contains("INVISIBILITY"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode hidden presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "hidden layer")
        .expect("decoded hidden presentation layer");
    assert_eq!(layer.visible, Some(false));
    assert_ne!(decoded.ir().model.bodies[0].visible, Some(false));
}

#[test]
fn writer_round_trips_empty_presentation_layer_label() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#unnamed").expect("identity grammar"),
        name: String::new(),
        description: Some("unnamed layer".into()),
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write empty-label presentation layer");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('','unnamed layer',"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode empty-label presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name.is_empty())
        .expect("decoded empty-label presentation layer");
    assert_eq!(layer.description.as_deref(), Some("unnamed layer"));
    assert_eq!(layer.visible, Some(false));
    assert!(matches!(
        layer.items.as_slice(),
        [PresentationItem::Body { .. }]
    ));
}

#[test]
fn analytic_surfaces_map_to_their_step_entities() {
    // Build one doc per analytic kind and check the keyword appears.
    let cases: Vec<(SurfaceGeometry, &str)> = vec![
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    5.0,
                )
                .unwrap(),
            )),
            "CYLINDRICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                    1.0,
                    0.5,
                )
                .unwrap(),
            )),
            "CONICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    4.0,
                )
                .unwrap(),
            )),
            "SPHERICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    3.0,
                    1.0,
                )
                .unwrap(),
            )),
            "TOROIDAL_SURFACE",
        ),
    ];
    for (geom, kw) in cases {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("test:model:surface#s").expect("identity grammar"),
            geometry: geom,
            source_object: None,
        });
        // Surfaces alone aren't reachable from a shell, so they won't be emitted
        // by the topology walk; emit directly via the geometry module instead.
        let s = emit_surface_only(&ir.model.surfaces[0].geometry);
        assert!(s.contains(kw), "missing {kw} in {s}");
    }
}

#[test]
fn analytic_surface_placements_preserve_orientation() {
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            4.0,
        )
        .unwrap(),
    ));
    let s = emit_surface_only(&geometry);
    assert!(s.contains("DIRECTION('',(0.,1.,0.))"));
    assert!(s.contains("DIRECTION('',(0.,0.,1.))"));
}

#[test]
fn nurbs_curve_non_rational_uses_with_knots() {
    let n = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let s = emit_curve_only(&CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(n)));
    assert!(s.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    // Clamped end knots collapse to multiplicity 3.
    assert!(s.contains("(3,3)"), "knot multiplicities: {s}");
    assert!(!s.contains("RATIONAL"));
}

#[test]
fn nurbs_curve_rational_uses_complex_form() {
    let n = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let s = emit_curve_only(&CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(n)));
    assert!(s.contains("RATIONAL_B_SPLINE_CURVE"));
    assert!(s.contains("BOUNDED_CURVE()"));
}

#[test]
pub(crate) fn nurbs_surface_grid_orientation_is_u_major() {
    let n = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let s = emit_surface_only(&SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(n)));
    assert!(s.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
}

#[test]
fn v1_document_uses_canonical_millimeter_unit() {
    let ir = unit_cube().expect("unit cube fixture is admitted");
    let s = export(&ir);
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
    assert!(!s.contains("CONVERSION_BASED_UNIT"));
}

#[test]
fn real_formatting_always_has_decimal_point() {
    // Coordinates like 10 must serialize as 10. (a Part 21 real), never 10.
    let s = export(&unit_cube().expect("unit cube fixture is admitted"));
    assert!(s.contains("10.")); // cube corner coordinate
    assert!(!s.contains("(10,")); // no bare integer coordinate
}

#[test]
fn writer_emits_both_carriers_for_mixed_general_bodies() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let edge = ir.model.edges[0].id.clone();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::General;
    {
        let members = vec![edge];
        ir.model.shells[0].edit_topology(|_, wire_edges, _| *wire_edges = members)
    }
    .unwrap();

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write mixed general body");
    assert!(!report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionNoConnectedEdgeSet.kind()
            && loss.message.contains("wire region")
    }));
    let text = String::from_utf8(output).expect("mixed general STEP is UTF-8");
    assert!(text.contains("SHELL_BASED_SURFACE_MODEL"));
    assert!(text.contains("EDGE_BASED_WIREFRAME_MODEL"));
}

#[test]
fn writer_orders_edge_loop_coedges_by_oriented_endpoints() {
    let mut source = unit_cube().expect("unit cube fixture is admitted");
    let loop_ = source
        .model
        .loops
        .iter_mut()
        .find(|loop_| loop_.coedges().len() >= 3)
        .expect("unit cube has an edge loop");
    let mut coedges = loop_.coedges().to_vec();
    coedges.swap(0, 1);
    let vertex_uses = loop_.anchored_vertex_uses().to_vec();
    loop_
        .replace_ring(
            &cadmpeg_test_support::service_decode_context(),
            coedges,
            vertex_uses,
        )
        .expect("reordered loop ring remains valid");

    let mut bytes = Vec::new();
    let report = write_step(
        &source,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("writer should recover a continuous loop order");
    assert!(!report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopNoContinuousOrdering.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("continuous vertex-to-vertex")
    }));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode reordered edge loops");
    assert_eq!(decoded.ir().model.faces.len(), source.model.faces.len());
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn writer_declares_each_supported_target_schema_exactly() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        let options = StepWriteOptions {
            ..StepWriteOptions::default()
        };
        let mut bytes = Vec::new();
        write_step(
            &unit_cube().expect("unit cube fixture is admitted"),
            &mut bytes,
            schema,
            &options,
        )
        .expect("write target schema");
        let text = std::str::from_utf8(&bytes).expect("ASCII STEP output");
        assert!(text.contains(&format!("FILE_SCHEMA(('{}'));", schema.file_schema())));
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode target-schema output");
    }
}

#[test]
fn exporting_a_salvaged_noncanonical_unit_repairs_partial_order() {
    let bytes = include_bytes!("../../../tests/fixtures/noncanonical_solid_angle.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode noncanonical unit fixture");
    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("export salvaged IR");

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(&output, crate::parse::parse_inner)
            .expect("parse repaired output");
    assert!(diagnostics.is_empty());
    let unit = exchange
        .records()
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "SOLID_ANGLE_UNIT")
        })
        .expect("exported solid-angle unit");
    assert_eq!(
        unit.partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>(),
        ["NAMED_UNIT", "SI_UNIT", "SOLID_ANGLE_UNIT"]
    );
}

/// The sweep vector's components are finite, and its length is past the
/// largest finite `f64`.
#[test]
fn a_linear_sweep_whose_magnitude_overflows_is_refused() {
    use cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction;
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap242_geometry.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode procedural geometry");
    let mut ir = source.ir().clone();
    let sweep = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.definition(),
                ProceduralSurfaceDefinition::LinearSweep(_)
            )
        })
        .expect("the fixture states a linear sweep");
    sweep.edit_definition(|definition| {
        let ProceduralSurfaceDefinition::LinearSweep(construction) = definition else {
            panic!("the selected surface is a linear sweep");
        };
        *construction = LinearSweepSurfaceConstruction::try_new(
            construction.directrix().clone(),
            Vector3::new(1.3e308, 1.3e308, 0.0),
        )
        .expect("finite components admit the sweep");
    });

    let mut bytes = Vec::new();
    let written = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    );
    let text = String::from_utf8_lossy(&bytes);
    let error = written.expect_err(&format!(
        "an overflowing sweep magnitude has no STEP real: {}",
        text.lines()
            .filter(|line| line.contains("VECTOR") || line.contains("SURFACE_OF_LINEAR"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    assert!(
        error
            .to_string()
            .contains("not implemented yet: STEP writer computed the non-finite real inf"),
        "{error}"
    );
}

/// The formatter states a finite number and refuses the section for any
/// other, where it wrote `0.` in its place.
#[test]
fn the_part21_real_formatter_refuses_a_non_finite_value() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let emitter = crate::writer::Emitter::new();
        assert_eq!(emitter.real(1.5), "1.5");
        assert_eq!(emitter.real(value), "", "{value}");
        let Err(error) = emitter.into_lines() else {
            panic!("a non-finite real refuses the section: {value}");
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::NotImplemented(_)),
            "{error}"
        );
    }
}

/// Decodes `records`, gives each procedural replica `transform`, and writes
/// the model.
fn replica_written_with(
    records: &str,
    transform: Transform,
) -> Result<String, cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProceduralSurfaceDefinition};

    let source = crate::test_support::exchange::decode_inline(records);
    let mut ir = source.ir().clone();
    let mut replicas = 0;
    for surface in &mut ir.model.procedural_surfaces {
        surface.edit_definition(|definition| {
            if let ProceduralSurfaceDefinition::Replica {
                transform: held, ..
            } = definition
            {
                *held = transform;
                replicas += 1;
            }
        });
    }
    for curve in &mut ir.model.procedural_curves {
        curve.edit_definition(|definition| {
            if let ProceduralCurveDefinition::Replica {
                transform: held, ..
            } = definition
            {
                *held = transform;
                replicas += 1;
            }
        });
    }
    assert_eq!(replicas, 1, "the exchange states one replica");
    let mut bytes = Vec::new();
    crate::export::write_step_outcome(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )?;
    Ok(String::from_utf8(bytes).expect("STEP output is UTF-8"))
}

const SURFACE_REPLICA_EXCHANGE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,2.,#4);
#8=SURFACE_REPLICA('',#6,#7);
#9=RECTANGULAR_TRIMMED_SURFACE('',#8,0.,1.,0.,1.,.T.,.T.);
#10=GEOMETRIC_SET('',(#9));
#11=SHAPE_REPRESENTATION('',(#10),#12);
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";

const CURVE_REPLICA_EXCHANGE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,2.);
#6=LINE('',#1,#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#8=CURVE_REPLICA('',#6,#7);
#9=TRIMMED_CURVE('',#8,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#10=GEOMETRIC_CURVE_SET('',(#9));
#11=SHAPE_REPRESENTATION('',(#10),$);";

/// A procedural replica refuses the file for `rows`.
fn assert_replica_is_refused(exchange: &str, rows: [[f64; 4]; 3]) {
    let written = replica_written_with(exchange, Transform::affine(rows).expect("affine"));
    let Err(error) = written else {
        panic!("{written:?}");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(_)),
        "{error}"
    );
}

const SHEAR: [[f64; 4]; 3] = [
    [1.0, 0.5, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A procedural replica whose transform is a similarity is written as a
/// replica.
#[test]
fn a_procedural_replica_with_a_similarity_is_written() {
    let similarity = Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("affine transform");
    for (exchange, replica) in [
        (SURFACE_REPLICA_EXCHANGE, "SURFACE_REPLICA"),
        (CURVE_REPLICA_EXCHANGE, "CURVE_REPLICA"),
    ] {
        let text = replica_written_with(exchange, similarity).expect("a similarity is written");
        assert!(text.contains(replica), "{text}");
    }
}

/// A sheared surface replica refuses the file, where it was written as a
/// different transform.
#[test]
fn a_surface_replica_refuses_a_shear() {
    assert_replica_is_refused(SURFACE_REPLICA_EXCHANGE, SHEAR);
}

/// A sheared curve replica refuses the file, where it was written as a
/// different transform.
#[test]
fn a_curve_replica_refuses_a_shear() {
    assert_replica_is_refused(CURVE_REPLICA_EXCHANGE, SHEAR);
}

/// A replica transform with a zero column refuses the file, where the column
/// was written as the direction `(0,0,1)`.
#[test]
fn a_replica_refuses_a_zero_column() {
    assert_replica_is_refused(
        CURVE_REPLICA_EXCHANGE,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ],
    );
}

#[test]
fn disconnected_wire_edges_export_in_separate_connected_sets() {
    let full = unit_cube().expect("cube");
    let mut ir = wire_body_ir(1.0);
    let second = full
        .model
        .edges
        .iter()
        .find(|edge| {
            ir.model
                .vertices
                .iter()
                .all(|v| v.id != edge.start && v.id != edge.end)
        })
        .expect("disconnected edge")
        .clone();
    let curve = second.curve().expect("curve");
    ir.model.curves.push(
        full.model
            .curves
            .iter()
            .find(|item| item.id == *curve)
            .expect("carrier")
            .clone(),
    );
    for vertex in full
        .model
        .vertices
        .iter()
        .filter(|v| v.id == second.start || v.id == second.end)
    {
        ir.model.points.push(
            full.model
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .expect("point")
                .clone(),
        );
        ir.model.vertices.push(vertex.clone());
    }
    ir.model.shells[0]
        .edit_topology(|_, edges, _| edges.push(second.id.clone()))
        .expect("wire topology");
    ir.model.edges.push(second);
    assert!(cadmpeg_ir::validate_neutral(&ir, Vec::new())
        .expect("neutral validation")
        .is_ok());
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("export");
    let (exchange, _) =
        crate::test_support::with_service_context(&output, crate::parse::parse_inner)
            .expect("exchange");
    assert_eq!(exchange.entities("CONNECTED_EDGE_SET").count(), 2);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("round trip");
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validation")
        .is_ok());
}
