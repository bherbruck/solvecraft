use cadmpeg_ir::geometry::CurveGeometry;
// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeMode, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};

use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, RegionId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Vertex};
use std::collections::HashSet;
use std::io::Cursor;

use crate::loss::StepLossCode;
use crate::reader::topology::{
    drop_committed_surfaces, pcurve_declared_endpoint_fit, pcurve_selection_seeds,
    pcurve_surface_closest, PCURVE_LOCUS_SAMPLE_COUNT,
};
use cadmpeg_ir::eval::model_surface_point_by_id;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;

fn surface_draft(id: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft
        .insert(
            Surface {
                id: SurfaceId::mint(id).expect("identity grammar"),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            },
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("insert surface into draft");
    draft
}

#[test]
fn cross_root_surface_filter_tracks_successful_commits_only() {
    let committed_id = "step:data:surface#implicit-face-1";
    let rejected_id = "step:data:surface#implicit-face-2";
    let mut ir = CadIr::empty();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

    session
        .commit_model(surface_draft(committed_id))
        .unwrap()
        .expect("first root commit");
    let mut second_root = surface_draft(committed_id);
    drop_committed_surfaces(&mut second_root, &mut session, &ctx).unwrap();
    assert!(second_root.model().surfaces.is_empty());

    let mut rejected_root = surface_draft(rejected_id);
    rejected_root
        .insert(
            Vertex {
                id: "step:data:vertex#rejected"
                    .try_into()
                    .expect("valid identity"),
                point: "step:data:point#missing"
                    .try_into()
                    .expect("valid identity"),
                tolerance: None,
            },
            &ctx,
        )
        .expect("insert invalid root reference");
    assert!(session.commit_model(rejected_root).unwrap().is_err());

    let mut later_root = surface_draft(rejected_id);
    drop_committed_surfaces(&mut later_root, &mut session, &ctx).unwrap();
    assert_eq!(later_root.model().surfaces.len(), 1);
}

#[test]
fn trimmed_pcurve_fit_uses_declared_endpoints() {
    let surface_id =
        SurfaceId::mint("step:data:surface#trimmed-endpoints").expect("identity grammar");
    let surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Trimmed(
        cadmpeg_ir::geometry::pcurve::TrimmedPcurve::try_new(
            [
                std::f64::consts::FRAC_PI_2,
                5.0 * std::f64::consts::FRAC_PI_2,
            ],
            true,
            Box::new(PcurveGeometry::Circle(
                cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 1.0),
                    1.0,
                )
                .unwrap(),
            )),
        )
        .unwrap(),
    );

    let fit = pcurve_declared_endpoint_fit(
        &cadmpeg_test_support::service_decode_context(),
        &ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex),
        &surface_id,
        &pcurve,
        [
            std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2,
        ],
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    )
    .expect("resource allocation did not fail")
    .expect("declared pcurve endpoints should be evaluable");

    assert!(fit <= 2.0 * f64::EPSILON);
}

#[test]
fn bounded_pcurve_search_can_miss_an_unsampled_exact_point() {
    let surface_id =
        SurfaceId::mint("step:data:surface#bounded-search-witness").expect("identity grammar");
    let surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: surface_geometry.clone(),
        source_object: None,
    });

    // The polar harmonic has a stationary chart tangent at seed 0. Its
    // exact point at pi is outside the default seed set, so the bounded
    // Newton loop cannot prove that the seed result is the global minimum.
    let pcurve = PcurveGeometry::PolarHarmonic(
        cadmpeg_ir::geometry::pcurve::PolarHarmonicPcurve::try_new(
            Point2::new(2.0, -1.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
            0.0,
            0.0,
            0.0,
        )
        .unwrap(),
    );
    let exact_parameter = std::f64::consts::PI;
    let exact_uv = cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &pcurve,
        exact_parameter,
    )
    .expect("witness pcurve is evaluable");
    let target = Point3::new(exact_uv.u, exact_uv.v, 0.0);
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let seeds = pcurve_selection_seeds(&index, &surface_id, &pcurve, &surface_geometry, &ctx)
        .expect("seed collection fits policy");
    assert_eq!(seeds, vec![0.0]);
    let bounded = pcurve_surface_closest(&ctx, &index, &surface_id, &pcurve, target, &seeds)
        .expect("resource allocation did not fail")
        .expect("bounded search returns an evaluated witness");
    assert!(bounded.0 > cadmpeg_ir::units::COINCIDENCE_TOLERANCE);
    let exact = cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &pcurve,
        exact_parameter,
    )
    .expect("exact point remains evaluable");
    assert!(Point3::new(exact.u, exact.v, 0.0).distance(target) <= f64::EPSILON);
}

#[test]
fn stale_trim_recovery_is_retained_above_step_tolerance() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
        "#70=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'distance_accuracy_value','');\n#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#70)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
    )
    .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);")
    .replace(
        "#54=LINE('',#51,#53);",
        "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.005)),.T.,.PARAMETER.);\n#71=LINE('',#51,#53);",
    );
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode stale trim with document tolerance");
    assert!((decoded.ir().tolerances.linear.get() - 0.1).abs() <= f64::EPSILON);
    let use_ = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .find(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
        .expect("stale trimmed pcurve use");
    assert_eq!(
        use_.parameter_range
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
        Some([0.0, 1.0])
    );
}

#[test]
fn finite_pcurve_admission_marks_unsampled_global_divergence() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_unsampled_divergence.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode unsampled-divergence pcurve witness");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#10")
        .expect("unsampled-divergence edge use");
    assert_eq!(edge_use.pcurves.len(), 1);
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#27")
        .expect("unsampled-divergence pcurve");
    let parameter_range = match &pcurve.geometry {
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            parameter_range.endpoints()
        }
        other => panic!("expected trimmed pcurve, got {other:?}"),
    };
    let surface_id = decoded
        .ir()
        .model
        .surfaces
        .first()
        .expect("pcurve plane")
        .id
        .clone();
    let (curve_center, curve_radius) = decoded
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let radius = circle_curve.radius().get();
                Some((center, radius))
            }
            _ => None,
        })
        .expect("3D circle carrier");
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    let point_set_residual = |fraction: f64| {
        let parameter = parameter_range[0].mul_add(1.0 - fraction, parameter_range[1] * fraction);
        let uv = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &pcurve.geometry,
            parameter,
        )
        .expect("evaluate pcurve");
        let mapped = model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            uv.u,
            uv.v,
        )
        .expect("map pcurve through plane");
        (mapped.distance(curve_center) - curve_radius).abs()
    };

    for sample in 0..PCURVE_LOCUS_SAMPLE_COUNT {
        let fraction = cadmpeg_core::convert::f64_from_index(sample).expect("test sample is exact")
            / cadmpeg_core::convert::f64_from_index(PCURVE_LOCUS_SAMPLE_COUNT - 1)
                .expect("test sample count is exact");
        assert!(point_set_residual(fraction) <= COINCIDENCE_TOLERANCE);
    }
    for gap in 0..(PCURVE_LOCUS_SAMPLE_COUNT - 1) {
        let fraction = (cadmpeg_core::convert::f64_from_index(gap).expect("test gap is exact")
            + 0.5)
            / cadmpeg_core::convert::f64_from_index(PCURVE_LOCUS_SAMPLE_COUNT - 1)
                .expect("test sample count is exact");
        assert!(point_set_residual(fraction) > 1.0);
    }

    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind())
        .expect("finite admission loss");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
}

/// Strict decode keeps a finitely admitted pcurve: the relation transfers the
/// source data with its verification status, and strict decode refuses only
/// CADIR-introduced substitution, salvage, and malformed structure.
#[test]
fn strict_decode_accepts_a_finitely_admitted_pcurve() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_unsampled_divergence.p21")),
            &options,
        )
        .expect("strict decode accepts an admitted pcurve");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#10")
        .expect("unsampled-divergence edge use");
    assert_eq!(edge_use.pcurves.len(), 1);
    let admissions = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind())
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), 1);
    assert_eq!(
        admissions[0].severity,
        cadmpeg_ir::report::Severity::Warning
    );
    assert_eq!(
        admissions[0].strict_consequence(),
        cadmpeg_ir::report::loss::StrictConsequence::Tolerate
    );
}

/// The document reports one admission warning for all admitted relations. The
/// fixture has two edges whose surface curve carries one exact pcurve on the
/// face plane, so the one warning counts two relations and names both.
#[test]
fn one_document_warning_counts_every_admitted_pcurve_relation() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp12_two_admissions.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode two-admission witness");

    for edge in ["step:data:edge#44", "step:data:edge#45"] {
        let edge_use = decoded
            .ir()
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.edge.as_str() == edge)
            .expect("admitted edge use");
        assert_eq!(edge_use.pcurves.len(), 1);
    }

    let admissions = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind())
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), 1);
    assert_eq!(
        admissions[0].severity,
        cadmpeg_ir::report::Severity::Warning
    );
    let message = admissions[0].message.as_str();
    assert!(message.contains("admits 2 pcurve relation(s)"), "{message}");
    assert!(
        message.contains("curve #42 on surface #11 at coedge use #48"),
        "{message}"
    );
    assert!(
        message.contains("curve #43 on surface #11 at coedge use #49"),
        "{message}"
    );
    assert!(!message.contains("more"), "{message}");
}

/// The document counts an admitted relation only when the body that holds it
/// reaches the document. The witness adds the edge loop as a second shell
/// member of the two-admission fixture. The first face admits both relations,
/// then the non-face member rejects the whole topology root, so the discarded
/// relations report nothing.
#[test]
fn a_discarded_topology_root_counts_no_admitted_relation() {
    let source = String::from_utf8(include_bytes!("data/tp12_two_admissions.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#55=OPEN_SHELL('',(#54));", "#55=OPEN_SHELL('',(#54,#52));");
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode discarded-root witness");

    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::TopologyRootRejected.kind() }));
    assert!(decoded.ir().model.coedges.is_empty());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PcurveGlobalFidelityUnproved.kind()));
}

#[test]
fn single_pcurve_with_unresolved_vertex_carrier_reports_loss() {
    let source = std::str::from_utf8(include_bytes!("data/tp09_divergent_interior.p21"))
        .expect("fixture is UTF-8")
        .replace("#7=VERTEX_POINT('',#4);", "#7=VERTEX_POINT('',#9);");
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode unresolved vertex carrier");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCandidatesCarrierUnresolved.kind()
            && loss.message.contains("coedge use #22")
    }));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::PcurveAssociationAmbiguous.kind() }));
}

#[test]
fn divergent_interior_pcurve_is_omitted_from_coedge() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_divergent_interior.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode divergent-interior pcurve witness");

    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("divergent pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveLocusDiscontinuous.kind()
            && loss.message.contains("bounded model-space locus")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#56"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn competing_same_surface_pcurves_remain_detached() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp09_competing_pcurves.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode competing pcurve witness");

    assert!(decoded.ir().model.pcurves.is_empty());
    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("competing pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("2 pcurves")
            && loss.message.contains("surface #28")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#56"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#69"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn assert_tp09_competing_pcurves_are_order_independent(source: &[u8]) {
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode competing pcurve witness");

    assert!(decoded.ir().model.pcurves.is_empty());
    let edge_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#19")
        .expect("competing pcurve edge use");
    assert!(edge_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("2 pcurves")
            && loss.message.contains("surface #28")
    }));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#56"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#69"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reordered_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_reordered.p21"
    ));
}

#[test]
fn near_tied_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_near_tied.p21"
    ));
}

#[test]
fn crossing_competing_same_surface_pcurves_remain_detached() {
    assert_tp09_competing_pcurves_are_order_independent(include_bytes!(
        "data/tp09_competing_pcurves_crossing.p21"
    ));
}

#[test]
fn shared_step_pcurve_mismatch_omits_optional_use() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc04_shared_pcurve.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode shared pcurve witness");

    let source = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#33")
        .expect("shared source pcurve");
    assert!(
        matches!(&source.geometry, PcurveGeometry::Trimmed(trimmed_pcurve)
        if {
            let parameter_range = trimmed_pcurve.parameter_range();
            trimmed_pcurve.same_sense() && (parameter_range.endpoints() == [0.0, 1.0])
        })
    );

    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| !pcurve.id.as_str().contains("-use-")));

    let first_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#42")
        .expect("first shared pcurve coedge");
    assert_eq!(first_use.pcurves.len(), 1);
    assert_eq!(first_use.pcurves[0].pcurve.as_str(), "step:data:pcurve#33");
    let second_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#45")
        .expect("second shared pcurve coedge");
    assert!(second_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #35")
            && loss.message.contains("surface #26")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reordered_shared_step_pcurve_mismatch_omits_optional_use() {
    let decoded = crate::StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc04_shared_pcurve_reordered.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode reordered shared pcurve witness");

    let source = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#33")
        .expect("reordered shared source pcurve");
    assert!(
        matches!(&source.geometry, PcurveGeometry::Trimmed(trimmed_pcurve)
        if {
            let parameter_range = trimmed_pcurve.parameter_range();
            trimmed_pcurve.same_sense() && (parameter_range.endpoints() == [0.0, 1.0])
        })
    );

    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| !pcurve.id.as_str().contains("-use-")));

    let uses = decoded
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge.as_str() == "step:data:edge#42")
        .map(|coedge| coedge.pcurves.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].len(), 1);
    assert_eq!(uses[0][0].pcurve.as_str(), "step:data:pcurve#33");
    let second_use = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge.as_str() == "step:data:edge#45")
        .expect("reordered second shared pcurve coedge");
    assert!(second_use.pcurves.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #35")
            && loss.message.contains("surface #26")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_surface_carrier_is_staged_once() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let surface = Surface {
        id: SurfaceId::mint("step:data:surface#shared").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let body_id = BodyId::mint("step:data:body#shared-surface").expect("identity grammar");
    let region_id = RegionId::mint("step:data:region#shared-surface").expect("identity grammar");
    let built = super::super::staged_topology(
        super::super::StagedTopologyParts {
            typed: HashSet::new(),
            vertices: Vec::new(),
            edges: Vec::new(),
            coedges: Vec::new(),
            loops: Vec::new(),
            faces: Vec::new(),
            surfaces: vec![surface.clone(), surface],
            shells: Vec::new(),
            region: Region {
                id: region_id.clone(),
                body: body_id.clone(),
                shells: Vec::new(),
            },
            body: Body {
                id: body_id,
                kind: BodyKind::Sheet,
                regions: vec![region_id],
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        },
        &ctx,
    )
    .expect("duplicate references to one source surface must stage");

    assert_eq!(built.draft.model().surfaces.len(), 1);
}
