// SPDX-License-Identifier: Apache-2.0
//! Geometry ownership and active domains at STEP export admission.
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use super::reports::edgeless_doc;
use crate::{export::write_step, loss::StepLossCode, StepSchema, StepWriteOptions};
use cadmpeg_ir::{
    geometry::{Curve, CurveGeometry, SolvedCurveGeometry},
    ids::CurveId,
    math::{Point3, Vector3},
    CadIr,
};

#[test]
fn support_ownership_survives_the_absence_of_projected_topology() {
    for role in [
        cadmpeg_ir::SourceGeometryRole::Support,
        cadmpeg_ir::SourceGeometryRole::Independent,
    ] {
        let mut ir = edgeless_doc();
        ir.model = Default::default();
        let mut surface = edgeless_doc().model.surfaces.remove(0);
        surface.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Iges,
            geometry_role: Some(role),
            object_id: cadmpeg_core::nonblank_literal!("D1"),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        });
        ir.model.surfaces.push(surface);
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let written = std::str::from_utf8(&bytes).unwrap();
        let support = role == cadmpeg_ir::SourceGeometryRole::Support;
        assert_eq!(written.contains("PLANE("), !support);
        assert_eq!(
            report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()),
            support
        );
    }
}

#[test]
fn failed_face_does_not_promote_its_surface_to_a_geometric_set() {
    let ir = edgeless_doc();
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("GEOMETRIC_SET("));
    assert!(!report.losses.is_empty());
}

#[test]
fn an_unwritten_vertex_does_not_promote_its_support_point() {
    let mut fixture = edgeless_doc();
    let mut ir = CadIr::empty();
    ir.model.points.push(fixture.model.points.remove(0));
    ir.model.vertices.push(fixture.model.vertices.remove(0));
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("GEOMETRIC_CURVE_SET("));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.message.contains(ir.model.points[0].id.as_str())
    }));
}

#[test]
fn independent_curve_preserves_its_active_parameter_interval() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("test:model:curve#active").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            ),
        )),
        parameter_range: cadmpeg_ir::topology::IncreasingParameterInterval::new([2.0, 3.0]),
        source_object: None,
    });
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let trimmed = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "TRIMMED_CURVE")
        .expect("active interval is encoded as a trimmed curve");
    for (slot, expected) in [(2, 2.0), (3, 3.0)] {
        assert_eq!(
            trimmed.parameters[slot],
            crate::parse::Value::List(vec![crate::parse::Value::Typed(
                "PARAMETER_VALUE".into(),
                Box::new(crate::parse::Value::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(expected).unwrap()
                ))
            )])
        );
    }
}

#[test]
fn independent_exact_surface_preserves_its_active_uv_rectangle() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
        surface_payloads::ExactSurfacePayload,
        ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let nurbs = NurbsSurface::from_lanes(
        &ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 5.0, 5.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 5.0, 5.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)],
                vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
    .unwrap();
    let mut ir = CadIr::empty();
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#active").unwrap();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            &ctx,
            &id,
            ProceduralSurface::new(
                "test:model:procedural_surface#active".try_into().unwrap(),
                ProceduralSurfaceDefinition::Exact(ExactSurfacePayload::from_legacy_intervals(
                    cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 3.0]).unwrap(),
                    cadmpeg_ir::topology::IncreasingParameterInterval::new([1.0, 4.0]).unwrap(),
                    0,
                    None,
                )),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let root = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "GEOMETRIC_SET")
        .unwrap();
    let crate::parse::Value::List(members) = &root.parameters[1] else {
        panic!("root members are a list");
    };
    assert_eq!(members.len(), 1);
    let crate::parse::Value::Reference(id) = members[0] else {
        panic!("root member is a reference");
    };
    let trimmed = &exchange.records()[&id].partials[0];
    assert_eq!(trimmed.name, "RECTANGULAR_TRIMMED_SURFACE");
    for (slot, expected) in [(2, 0.0), (3, 3.0), (4, 1.0), (5, 4.0)] {
        assert_eq!(
            trimmed.parameters[slot],
            crate::parse::Value::Real(cadmpeg_ir::scalar::FiniteReal::new(expected).unwrap())
        );
    }
}

#[test]
fn procedural_support_is_not_an_export_root_even_when_construction_fails() {
    use cadmpeg_ir::geometry::{
        surface_payloads::SubsetSurfaceConstruction, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    for senses in [Some(true), None] {
        for independent in [false, true] {
            let mut ir = CadIr::empty();
            let mut support = edgeless_doc().model.surfaces.remove(0);
            if independent {
                support.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Fcstd,
                    geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Independent),
                    object_id: cadmpeg_core::nonblank_literal!("authored-plane"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                });
            }
            let construction = ProceduralSurface::new(
                "test:model:procedural_surface#subset".try_into().unwrap(),
                ProceduralSurfaceDefinition::Subset(
                    SubsetSurfaceConstruction::try_new(
                        support.id.clone(),
                        [[0.0, 1.0], [0.0, 1.0]],
                        senses,
                        senses,
                        None,
                    )
                    .unwrap(),
                ),
                None,
            );
            ir.model.surfaces.push(support);
            ir.model.surfaces.push(Surface {
                id: "test:model:surface#subset".try_into().unwrap(),
                geometry: SurfaceGeometry::Procedural {
                    construction: construction.id.clone(),
                    cache: None,
                },
                source_object: None,
            });
            ir.model.procedural_surfaces.push(construction);
            let mut bytes = Vec::new();
            let report = write_step(
                &ir,
                &mut bytes,
                StepSchema::Ap242Edition3,
                &StepWriteOptions::default(),
            )
            .unwrap();
            let (exchange, _) =
                crate::test_support::with_service_context(&bytes, crate::parse::parse_inner)
                    .unwrap();
            let members: Vec<_> = exchange
                .records()
                .values()
                .flat_map(|record| &record.partials)
                .filter(|partial| partial.name == "GEOMETRIC_SET")
                .flat_map(|partial| match &partial.parameters[1] {
                    crate::parse::Value::List(items) => items.as_slice(),
                    _ => panic!("root members are a list"),
                })
                .map(|value| match value {
                    crate::parse::Value::Reference(id) => {
                        exchange.records()[id].partials[0].name.as_str()
                    }
                    _ => panic!("root member is a reference"),
                })
                .collect();
            assert_eq!(members.contains(&"PLANE"), independent);
            assert_eq!(
                members.contains(&"RECTANGULAR_TRIMMED_SURFACE"),
                senses.is_some()
            );
            assert_eq!(
                report
                    .losses
                    .iter()
                    .any(|loss| loss.code == StepLossCode::ProceduralDefinitionNotWritten.kind()),
                senses.is_none()
            );
            assert!(!report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::ProceduralReducedToCarrier.kind()));
            if senses.is_some() {
                assert!(!report
                    .losses
                    .iter()
                    .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
            }
        }
    }
}

#[test]
fn standalone_subset_curve_does_not_export_its_untrimmed_basis_as_a_root() {
    use cadmpeg_ir::geometry::{
        curve_payloads::SubsetCurveConstruction, ProceduralCurve, ProceduralCurveDefinition,
    };
    let mut ir = CadIr::empty();
    let basis: CurveId = "test:model:curve#basis".try_into().unwrap();
    let construction = ProceduralCurve::new(
        "test:model:procedural_curve#subset".try_into().unwrap(),
        ProceduralCurveDefinition::Subset(
            SubsetCurveConstruction::try_new(basis.clone(), [2.0, 3.0], true, None).unwrap(),
        ),
    );
    ir.model.curves.push(Curve {
        id: basis,
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            ),
        )),
    });
    ir.model.curves.push(Curve {
        id: "test:model:curve#subset".try_into().unwrap(),
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Procedural {
            construction: construction.id.clone(),
            cache: None,
        },
    });
    ir.model.procedural_curves.push(construction);
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let root = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "GEOMETRIC_CURVE_SET")
        .unwrap();
    let crate::parse::Value::List(members) = &root.parameters[1] else {
        panic!("root list");
    };
    assert_eq!(members.len(), 1);
    let crate::parse::Value::Reference(id) = members[0] else {
        panic!("root reference");
    };
    assert_eq!(exchange.records()[&id].partials[0].name, "TRIMMED_CURVE");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
}

#[test]
fn failed_composite_curve_does_not_export_its_children_as_roots() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition,
    };
    for missing_child in [false, true] {
        let mut ir = CadIr::empty();
        let child: CurveId = "test:model:curve#child".try_into().unwrap();
        ir.model.curves.push(Curve {
            id: child.clone(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                    cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))
                        .unwrap(),
                ),
            )),
        });
        let mut segments = vec![CompositeCurveSegment {
            curve: child,
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }];
        if missing_child {
            segments.push(CompositeCurveSegment {
                curve: "test:model:curve#missing".try_into().unwrap(),
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            });
        }
        ir.model.curves.push(Curve {
            id: "test:model:curve#composite".try_into().unwrap(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: CompositeCurveSegments::try_from(segments).unwrap(),
                self_intersect: Some(false),
            }),
        });
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let (exchange, _) =
            crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
        let root = exchange
            .records()
            .values()
            .flat_map(|record| &record.partials)
            .find(|partial| partial.name == "GEOMETRIC_CURVE_SET");
        if missing_child {
            assert!(root.is_none());
            assert!(report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
        } else {
            let crate::parse::Value::List(members) = &root.unwrap().parameters[1] else {
                panic!("root list");
            };
            assert_eq!(members.len(), 1);
            let crate::parse::Value::Reference(id) = members[0] else {
                panic!("root reference");
            };
            assert_eq!(exchange.records()[&id].partials[0].name, "COMPOSITE_CURVE");
            assert!(!report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
        }
    }
}
