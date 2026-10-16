// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::Point2;

fn with_context<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    run(&ctx)
}
fn plane() -> (cadmpeg_ir::CadIr, SurfaceId) {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = SurfaceId::mint("test:audit:surface#1").unwrap();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    (ir, id)
}
#[test]
fn numerical_0922b_pcurve_knot_units() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    for d in [1., 1e9] {
        let p = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0., 0., d, d],
                vec![Point2::new(0., 0.), Point2::new(1., 0.)],
                None,
                false,
            )
            .expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let seeds = with_context(|ctx| {
            pcurve_selection_seeds(&index, &id, &p, &ir.model.surfaces[0].geometry, ctx)
        })
        .expect("seed collection fits policy");
        let r = pcurve_surface_closest(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &id,
            &p,
            Point3::new(0.3, 0., 0.),
            &seeds,
        )
        .expect("resource allocation did not fail")
        .unwrap();
        println!("STEP d{d:e}, result{r:?}, x={}", r.1 / d);
        assert!(r.0 < 1e-14);
        assert!((r.1 / d - 0.3).abs() < 1e-14);
    }
}

#[test]
fn pcurve_selection_keeps_interior_knots_and_seeds_in_a_wide_finite_domain() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![-f64::MAX, -f64::MAX, 0.0, f64::MAX, f64::MAX],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(2.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    };
    let mut fractions = Vec::new();
    with_context(|ctx| {
        pcurve_parameter_break_fractions(&pcurve, [-f64::MAX, f64::MAX], &mut fractions, ctx)
    })
    .expect("break fractions fit policy");
    assert_eq!(fractions, vec![0.5]);
    let seeds = with_context(|ctx| {
        pcurve_selection_seeds(&index, &id, &pcurve, &ir.model.surfaces[0].geometry, ctx)
    })
    .expect("seed collection fits policy");
    assert!(seeds
        .iter()
        .any(|seed| (seed / f64::MAX + 0.5).abs() < f64::EPSILON));
    assert!(seeds.iter().all(|seed| seed.is_finite()));
}

#[test]
fn periodic_surface_selection_keeps_quarter_seeds_across_a_wide_domain() {
    let max = f64::MAX;
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = SurfaceId::mint("test:audit:surface#wide-periodic").expect("surface id");
    let surface = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![-max, -max, max, max], true),
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
    .expect("wide periodic surface");
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("line pcurve"),
    );
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let seeds = with_context(|ctx| {
        pcurve_selection_seeds(&index, &id, &pcurve, &ir.model.surfaces[0].geometry, ctx)
    })
    .expect("seed collection fits policy");
    assert!(seeds
        .iter()
        .any(|seed| (seed / max + 0.5).abs() <= 8.0 * f64::EPSILON));
    assert!(seeds
        .iter()
        .any(|seed| (seed / max - 0.5).abs() <= 8.0 * f64::EPSILON));
}

#[test]
fn pcurve_locus_accepts_a_wide_finite_line_parameter_interval() {
    let (mut ir, surface_id) = plane();
    let curve_id = CurveId::from(ids::data(kind!("curve"), 54));
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        parameter_range: None,
        id: curve_id,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("finite line"),
        )),
        source_object: None,
    });
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let (exchange, _) = crate::test_support::with_service_context(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"),
        crate::parse::parse_inner,
    )
    .expect("STEP fixture parses");
    let edge = EdgeDef::Curve {
        start: 1,
        end: 2,
        curve: 54,
        same: true,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("finite pcurve"),
    );
    let lower = -9.0e307;
    let upper = 9.0e307;
    assert!(with_context(|ctx| pcurve_locus_witness(
        &index,
        &exchange,
        &edge,
        &surface_id,
        &pcurve,
        super::super::PcurveWitness {
            endpoint: PcurveEndpointFit {
                start_parameter: lower,
                end_parameter: upper,
                max_residual: 0.0,
            },
            curve_start: Point3::new(lower, 0.0, 0.0),
            curve_end: Point3::new(upper, 0.0, 0.0),
            bound: COINCIDENCE_TOLERANCE
        },
        ctx
    ))
    .expect("resource allocation did not fail"));
}

#[test]
fn pcurve_locus_fractions_refuse_collection_limit() {
    let (mut ir, surface_id) = plane();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        parameter_range: None,
        id: CurveId::from(ids::data(kind!("curve"), 54)),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("finite line"),
        )),
        source_object: None,
    });
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#54=LINE('',#55,#56);#55=DUMMY();#56=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid line reference");
    let edge = EdgeDef::Curve {
        start: 1,
        end: 2,
        curve: 54,
        same: true,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("finite pcurve"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(
        matches!(pcurve_locus_witness(&index, &exchange, &edge, &surface_id, &pcurve, super::super::PcurveWitness { endpoint: PcurveEndpointFit { start_parameter: 0.0, end_parameter: 1.0, max_residual: 0.0 }, curve_start: Point3::new(0.0, 0.0, 0.0), curve_end: Point3::new(1.0, 0.0, 0.0), bound: COINCIDENCE_TOLERANCE }, &ctx), Err(PcurveSelectionFailure::Resource(CodecError::ResourceLimit(refusal)))
        if refusal.operation == "step_pcurve_locus_fractions")
    );
}

#[test]
fn pcurve_locus_finds_an_interior_curve_branch_near_the_float_limit() {
    let (mut ir, surface_id) = plane();
    let curve_id = CurveId::from(ids::data(kind!("curve"), 54));
    let lower = f64::MAX * 0.5;
    let upper = f64::MAX;
    let control_count = 2049;
    let mut knots = vec![lower, lower];
    knots.extend((1..control_count - 1).map(|index| {
        cadmpeg_ir::math::interpolate(
            lower,
            upper,
            cadmpeg_core::convert::f64_from_index(index).expect("test index is exact")
                / cadmpeg_core::convert::f64_from_index(control_count - 1)
                    .expect("test control count is exact"),
        )
        .expect("finite interior knot")
        .get()
    }));
    knots.extend([upper, upper]);
    let controls = (0..control_count)
        .map(|index| Point3::new(if index == control_count / 2 { 1.0 } else { 0.0 }, 0.0, 0.0))
        .collect();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                knots,
                controls,
                None,
                false,
            )
            .expect("fixture constructor admission")
            .expect("finite many-span curve"),
        )),
        source_object: None,
    });
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let midpoint = lower.midpoint(upper);
    for x in [0.5, 0.6] {
        assert!(curve_parameter_near_point(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &curve_id,
            Point3::new(x, 0.0, 0.0),
            &[midpoint],
            COINCIDENCE_TOLERANCE,
        )
        .expect("resource allocation did not fail")
        .is_some());
    }
    let (exchange, _) = crate::test_support::with_service_context(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"),
        crate::parse::parse_inner,
    )
    .expect("STEP fixture parses");
    let edge = EdgeDef::Curve {
        start: 1,
        end: 2,
        curve: 54,
        same: true,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("finite pcurve"),
    );
    assert!(with_context(|ctx| pcurve_locus_witness(
        &index,
        &exchange,
        &edge,
        &surface_id,
        &pcurve,
        super::super::PcurveWitness {
            endpoint: PcurveEndpointFit {
                start_parameter: 0.5,
                end_parameter: 0.6,
                max_residual: 0.0,
            },
            curve_start: Point3::new(0.5, 0.0, 0.0),
            curve_end: Point3::new(0.6, 0.0, 0.0),
            bound: COINCIDENCE_TOLERANCE
        },
        ctx
    ))
    .expect("resource allocation did not fail"));
}

#[test]
fn numerical_0922b_pcurve_retains_finite_seed_when_step_overflows() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(0., 0.), Point2::new(1e-200, 0.)],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    };
    assert_eq!(
        mapped_pcurve_closest(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &id,
            &pcurve,
            Point3::new(1e200, 0., 0.),
            0.
        )
        .expect("resource allocation did not fail"),
        Some((1e200, 0.))
    );
}

/// A plane whose origin x coordinate is `origin_x`.
fn plane_at(origin_x: f64) -> (cadmpeg_ir::CadIr, SurfaceId) {
    let (mut ir, id) = plane();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(origin_x, 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
    ));
    (ir, id)
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_end_is_measured_at_its_finite_end() {
    // On the plane at x = MAX, the line end u = MAX has no finite point and
    // the start u = -MAX maps to the model origin.
    let (ir, id) = plane_at(f64::MAX);
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        )
        .expect("resource allocation did not fail"),
        Some(0.)
    );
}

#[test]
fn the_mapped_pcurve_search_halves_a_step_whose_point_overflows() {
    // On the plane at x = 1.78e308 the parabola `(t^2, t)` overflows for t
    // above about 1.33e153. The Newton step from t = 1e152 toward the point
    // at t = 1.2e153 lands at about 7.25e153; halving it returns the search
    // to the finite range.
    let (ir, id) = plane_at(1.78e308);
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let parabola = PcurveGeometry::Parabola(
        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(0., 1.),
            0.25,
        )
        .unwrap(),
    );
    let target_parameter = 1.2e153;
    let target = Point3::new(
        1.78e308 + target_parameter * target_parameter,
        target_parameter,
        0.,
    );
    let (error, parameter) = mapped_pcurve_closest(
        &cadmpeg_test_support::service_decode_context(),
        &index,
        &id,
        &parabola,
        target,
        1e152,
    )
    .expect("resource allocation did not fail")
    .unwrap();
    assert!(
        (parameter / target_parameter - 1.).abs() < 1e-6,
        "{parameter}"
    );
    assert!(error < 1e300, "{error}");
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_placed_end_misses_by_an_infinite_distance() {
    // The plane through the origin, placed by a transform that adds the
    // largest finite x coordinate: the line end u = MAX has no finite point
    // and the start u = -MAX maps to the model origin.
    let (mut ir, id) = plane();
    let SurfaceGeometry::Solved(plane) = ir.model.surfaces[0].geometry.clone() else {
        panic!("the plane fixture is solved");
    };
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(plane),
            cadmpeg_ir::transform::Transform::affine([
                [1., 0., 0., f64::MAX],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
            ])
            .unwrap(),
        )
        .unwrap(),
    ));
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        )
        .expect("resource allocation did not fail"),
        Some(f64::INFINITY)
    );
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_line_end_is_measured_at_its_finite_end() {
    // The line reaches u = MAX + MAX at its end, which the plane maps to no
    // finite point, and u = 0 at its start, which it maps to the origin.
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &cadmpeg_test_support::service_decode_context(),
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        )
        .expect("resource allocation did not fail"),
        Some(0.)
    );
}

#[test]
fn pcurve_selection_helpers_preserve_session_depth_refusal() {
    use cadmpeg_core::decode::ResourceDimension;
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid line"),
    );
    for seeded in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let target = Point3::new(0.5, 0.0, 0.0);
        let limit = if seeded {
            pcurve_surface_closest(&ctx, &index, &id, &pcurve, target, &[0.0])
        } else {
            mapped_pcurve_closest(&ctx, &index, &id, &pcurve, target, 0.0)
        }
        .expect_err("first seed or inverse step charges work");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
        assert_eq!(
            limit.operation,
            if seeded {
                "step pcurve seed visit"
            } else {
                "step pcurve inverse step"
            }
        );
        assert_eq!(
            ctx.charge_work_limit(0, "observe selection refusal"),
            Err(limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let limit = pcurve_selection_uv(&ctx, &pcurve, 0.5).expect_err("first evaluator frame refuses");
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
    assert_eq!(
        ctx.charge_work_limit(0, "observe selection refusal"),
        Err(limit)
    );
    for directed in [false, true] {
        let result = if directed {
            pcurve_declared_endpoint_fit_directed(
                &ctx,
                &index,
                &id,
                &pcurve,
                [0.0, 1.0],
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )
        } else {
            pcurve_declared_endpoint_fit(
                &ctx,
                &index,
                &id,
                &pcurve,
                [0.0, 1.0],
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )
        };
        assert_eq!(result, Err(limit));
    }
    assert_eq!(
        pcurve_surface_closest(&ctx, &index, &id, &pcurve, Point3::new(0.5, 0.0, 0.0), &[]),
        Err(limit)
    );
    assert_eq!(
        mapped_pcurve_closest(
            &ctx,
            &index,
            &id,
            &pcurve,
            Point3::new(0.5, 0.0, 0.0),
            f64::NAN
        ),
        Err(limit)
    );
}

#[test]
fn certified_pcurve_endpoint_does_not_repeat_tangents_or_seed_searches() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert_eq!(
        pcurve_surface_closest(
            &ctx,
            &index,
            &id,
            &pcurve,
            Point3::new(0.0, 0.0, 0.0),
            &[0.0; 1_000],
        )
        .unwrap(),
        Some((0.0, 0.0))
    );
    assert!(ctx.finish_session().is_ok());
    // The parameter is a forward-evaluated witness, not a seed-only guess.
    let off_surface = mapped_pcurve_closest(
        &cadmpeg_test_support::service_decode_context(),
        &index,
        &id,
        &pcurve,
        Point3::new(0.0, 0.0, 1.0),
        0.0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(off_surface, (1.0, 0.0));
}

#[test]
fn selection_domains_use_indexed_constructions_and_caller_work_limits() {
    let (mut ir, support) = plane();
    let mut subset = ir.model.surfaces[0].clone();
    subset.id = SurfaceId::mint("test:model:surface#subset").unwrap();
    let id = subset.id.clone();
    ir.model.surfaces.push(subset);
    ir.model
        .add_procedural_surface(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &id,
            cadmpeg_ir::geometry::ProceduralSurface::new(
                cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:procedural-surface#subset")
                    .unwrap(),
                ProceduralSurfaceDefinition::Subset(
                    cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
                        support,
                        [[2.0, 5.0], [4.0, 9.0]],
                        None,
                        None,
                        None,
                    )
                    .unwrap(),
                ),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    with_context(|ctx| {
        assert_eq!(
            surface_selection_parameter_domains(&index, &id, &ir.model.surfaces[1].geometry, ctx)
                .unwrap(),
            [Some([0.0, 3.0]), Some([0.0, 5.0])]
        );
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(matches!(surface_selection_parameter_domains(
        &index, &id, &ir.model.surfaces[1].geometry, &ctx),
        Err(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn established_curve_locus_witness_does_not_repeat_other_seed_inversions() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let curve = &ir.model.curves[0];
    let index = ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let point = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &index,
        &curve.id,
        0.0,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let seeds = (0..100)
        .map(|seed| cadmpeg_core::convert::f64_from_index(seed).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        curve_parameter_near_point(
            &ctx,
            &index,
            &curve.id,
            point.get(),
            &seeds,
            COINCIDENCE_TOLERANCE
        )
        .unwrap(),
        Some(0.0)
    );
}
