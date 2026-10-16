// SPDX-License-Identifier: Apache-2.0
use super::{curve, curve_is_supported};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

#[test]
fn rejects_transform_that_step_operator_cannot_represent() {
    let anisotropic = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ])
    .expect("affine transform");
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            anisotropic,
        )
        .expect("placed curve"),
    ));

    assert!(!curve_is_supported(&curve));
}

#[test]
fn a_transformed_composite_curve_is_an_omitted_carrier() {
    let composite = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![cadmpeg_ir::geometry::CompositeCurveSegment {
            curve: cadmpeg_ir::ids::CurveId::mint("step:data:curve#1").expect("curve id"),
            same_sense: true,
            transition: cadmpeg_ir::geometry::CompositeCurveTransition::Continuous,
        }]
        .try_into()
        .expect("nonempty composite segments"),
        self_intersect: None,
    });
    let transformed = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(composite.solved().expect("solved carrier").clone()),
            Transform::identity(),
        )
        .expect("placed curve"),
    ));

    let mut emitter = crate::writer::Emitter::new();
    assert!(curve(&mut emitter, transformed.solved().expect("solved carrier")).is_none());
    assert!(!curve_is_supported(&transformed));
}

#[test]
fn numerical_audit_pcurve_keeps_large_finite_direction_and_magnitude() {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;
    let mut emitter = crate::writer::Emitter::new();
    let line = LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0e200, 0.0)).unwrap();
    assert!(super::pcurve(&mut emitter, &PcurveGeometry::Line(line)).is_some());
    let lines = emitter.into_lines().expect("finite reals");
    assert!(lines
        .iter()
        .any(|line| line.contains("DIRECTION('',(1.,0.))")));
    assert!(lines.iter().any(|line| line.contains(&format!(
        "VECTOR('',#2,{})",
        crate::writer::Emitter::new().real(1.0e200)
    ))));
    let mut emitter = crate::writer::Emitter::new();
    let line = LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(f64::MAX, f64::MAX)).unwrap();
    assert!(super::pcurve(&mut emitter, &PcurveGeometry::Line(line)).is_none());
}

#[test]
fn conical_surface_emits_a_positive_half_angle_and_reversed_axis() {
    use super::surface;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    let cone = cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
        1.0,
        -0.715_584_993_317_674_8,
    )
    .unwrap();
    let mut emitter = crate::writer::Emitter::new();
    assert!(surface(&mut emitter, &SolvedSurfaceGeometry::Cone(cone)).is_some());
    let lines = emitter.into_lines().expect("finite reals");
    let emitted = lines
        .iter()
        .find(|line| line.contains("CONICAL_SURFACE("))
        .expect("the cone emits a CONICAL_SURFACE carrier");
    assert!(emitted.ends_with(&format!(
        ",{},{});",
        crate::writer::Emitter::new().real(5.0),
        crate::writer::Emitter::new().real(0.715_584_993_317_674_8)
    )));
    assert!(lines
        .iter()
        .any(|line| line.contains("DIRECTION('',(-0.,-0.,-1.))")));
}

/// The deepest chain of affine placements the writer accepts over one basis
/// carrier.
const ACCEPTED_PLACEMENTS: usize = cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING;

fn placed_plane(
    placements: usize,
) -> Result<cadmpeg_ir::geometry::SolvedSurfaceGeometry, &'static str> {
    use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry};
    let mut geometry = SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a unit-axis plane"),
    );
    for _ in 0..placements {
        geometry = SolvedSurfaceGeometry::Transformed(PlacedSurface::try_new(
            Box::new(geometry),
            Transform::identity(),
        )?);
    }
    Ok(geometry)
}

#[test]
fn a_surface_placement_chain_past_the_bound_is_unwritable() {
    use super::{emitted_basis, surface, surface_is_supported};

    let accepted = placed_plane(ACCEPTED_PLACEMENTS).expect("admitted nesting");
    assert!(surface_is_supported(&accepted));
    assert!(matches!(
        emitted_basis(&accepted),
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(_)
    ));
    let mut emitter = crate::writer::Emitter::new();
    assert!(surface(&mut emitter, &accepted).is_some());

    // One placement deeper is unwritable because it is unbuildable.
    assert_eq!(
        placed_plane(ACCEPTED_PLACEMENTS + 1),
        Err("PlacedSurface.basis nests past the admitted inline basis depth")
    );
}

fn placed_line(placements: usize) -> Result<CurveGeometry, &'static str> {
    let mut geometry = SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a unit-direction line"),
    );
    for _ in 0..placements {
        geometry = SolvedCurveGeometry::Transformed(cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(geometry),
            Transform::identity(),
        )?);
    }
    Ok(CurveGeometry::Solved(geometry))
}

#[test]
fn a_curve_placement_chain_past_the_bound_is_unwritable() {
    let accepted = placed_line(ACCEPTED_PLACEMENTS).expect("admitted nesting");
    assert!(curve_is_supported(&accepted));
    let mut emitter = crate::writer::Emitter::new();
    assert!(curve(&mut emitter, accepted.solved().expect("solved carrier")).is_some());

    // One placement deeper is unwritable because it is unbuildable.
    assert_eq!(
        placed_line(ACCEPTED_PLACEMENTS + 1),
        Err("PlacedCurve.basis nests past the admitted inline basis depth")
    );
}

fn placed_pcurve(
    placements: usize,
) -> Result<cadmpeg_ir::geometry::pcurve::PcurveGeometry, &'static str> {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::transform::Transform2;
    let mut geometry = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
            .expect("a unit-direction parameter-space line"),
    );
    for _ in 0..placements {
        geometry = PcurveGeometry::Transformed(PlacedPcurve::try_new(
            Box::new(geometry),
            Transform2::identity(),
        )?);
    }
    Ok(geometry)
}

#[test]
fn a_pcurve_placement_chain_past_the_bound_is_unwritable() {
    use super::pcurve;

    let accepted = placed_pcurve(ACCEPTED_PLACEMENTS).expect("admitted nesting");
    let mut emitter = crate::writer::Emitter::new();
    assert!(pcurve(&mut emitter, &accepted).is_some());

    // One carrier deeper is unwritable because it is unbuildable.
    assert_eq!(
        placed_pcurve(ACCEPTED_PLACEMENTS + 1),
        Err("PlacedPcurve.basis nests past the admitted inline basis depth")
    );
}

#[test]
fn small_shears_are_not_similarities() {
    use super::{Transform, Transform2};
    for a in [1e-10, 1., 1e200] {
        let shear = Transform::affine([
            [a, 0.5 * a, 0., 0.],
            [0., 0.75_f64.sqrt() * a, 0., 0.],
            [0., 0., a, 0.],
        ])
        .unwrap();
        let shear2 = Transform2::affine([[a, 0.5 * a, 0.], [0., 0.75_f64.sqrt() * a, 0.]]).unwrap();
        assert!(super::similarity_transform(&shear).is_none());
        assert!(!super::similarity_transform_2d(&shear2));
        let uniform =
            Transform::affine([[a, 0., 0., 0.], [0., a, 0., 0.], [0., 0., a, 0.]]).unwrap();
        assert!(super::similarity_transform(&uniform).is_some());
    }
}

/// A vector of finite components whose length overflows keeps its
/// orientation, where it became the zero `DIRECTION`.
#[test]
fn a_direction_whose_length_overflows_keeps_its_orientation() {
    const EPS_UNIT_COMPONENT: f64 = 1.0e-15;
    let mut emitter = crate::writer::Emitter::new();
    let direction =
        cadmpeg_ir::units::DirectionAboveEpsilon::new(Vector3::new(1.3e308, 1.3e308, 0.0))
            .expect("finite nonzero direction");
    super::direction(&mut emitter, direction);
    let lines = emitter.into_lines().expect("finite reals");
    let [line] = lines.as_slice() else {
        panic!("one DIRECTION record: {lines:?}");
    };
    let components = line
        .split_once("DIRECTION('',(")
        .and_then(|(_, rest)| rest.split_once("))"))
        .map(|(components, _)| {
            components
                .split(',')
                .map(|value| value.parse::<f64>().expect("a Part 21 real"))
                .collect::<Vec<_>>()
        })
        .expect("a DIRECTION record");
    assert_eq!(components.len(), 3, "{line}");
    assert!((components[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < EPS_UNIT_COMPONENT);
    assert!((components[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < EPS_UNIT_COMPONENT);
    assert_eq!(components[2], 0.0);
}

#[test]
fn a_zero_vector_cannot_be_emitted_as_a_direction() {
    assert!(cadmpeg_ir::units::DirectionAboveEpsilon::new(Vector3::new(0.0, 0.0, 0.0)).is_none());
}

/// The section written for one transformation operator over `rows`.
fn operator_section(rows: [[f64; 4]; 3]) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let transform = Transform::affine(rows).expect("affine transform");
    let mut emitter = crate::writer::Emitter::new();
    super::transformation_operator(&mut emitter, transform);
    emitter.into_lines()
}

/// A transform with no `CARTESIAN_TRANSFORMATION_OPERATOR_3D` statement
/// refuses the section.
fn assert_operator_is_refused(rows: [[f64; 4]; 3]) {
    let written = operator_section(rows);
    let Err(error) = written else {
        panic!("{written:?}");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(_)),
        "{error}"
    );
}

/// `CARTESIAN_TRANSFORMATION_OPERATOR_3D` states a similarity: three axis
/// directions and one scale.
#[test]
fn a_transformation_operator_states_a_similarity() {
    let lines = operator_section([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("a similarity is written");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("CARTESIAN_TRANSFORMATION_OPERATOR_3D")),
        "{lines:?}"
    );
}

/// A shear is refused, where it was written as the rotation of its
/// normalized columns.
#[test]
fn a_transformation_operator_refuses_a_shear() {
    assert_operator_is_refused([
        [1.0, 0.5, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
}

/// A non-uniform scale is refused, where it was written with the scale of
/// its first column.
#[test]
fn a_transformation_operator_refuses_a_non_uniform_scale() {
    assert_operator_is_refused([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ]);
}

/// A zero column is refused, where it was written as the direction
/// `(0,0,1)`.
#[test]
fn a_transformation_operator_refuses_a_zero_column() {
    assert_operator_is_refused([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
    ]);
}
