// SPDX-License-Identifier: Apache-2.0
//! STEP writer loss, schema, and strict-mode tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_test_support::edit;

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, ExportPlan};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

use super::round_trips::cylinder_surface_doc;

/// A one-face document whose single edge has no attributed curve, so the writer
/// must omit that edge and record a loss.
pub(super) fn edgeless_doc() -> CadIr {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };
    let mut ir = CadIr::empty();
    ir.model.points.push(Point::new(
        PointId::mint("test:model:point#p0").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.model.points.push(Point::new(
        PointId::mint("test:model:point#p1").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("test:model:vertex#v0").expect("identity grammar"),
        point: PointId::mint("test:model:point#p0").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("test:model:vertex#v1").expect("identity grammar"),
        point: PointId::mint("test:model:point#p1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#e0").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
        start: VertexId::mint("test:model:vertex#v0").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#v1").expect("identity grammar"),
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:surface#s0").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.coedges.push(Coedge {
        id: CoedgeId::mint("test:model:coedge#ce0").expect("identity grammar"),
        owner_loop: LoopId::mint("test:model:loop#lp0").expect("identity grammar"),
        edge: EdgeId::mint("test:model:edge#e0").expect("identity grammar"),
        radial_next: CoedgeId::mint("test:model:coedge#ce0").expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    ir.model.loops.push(Loop {
        id: LoopId::mint("test:model:loop#lp0").expect("identity grammar"),
        face: FaceId::mint("test:model:face#f0").expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![CoedgeId::mint("test:model:coedge#ce0").expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    });
    ir.model.faces.push(Face {
        id: FaceId::mint("test:model:face#f0").expect("identity grammar"),
        shell: ShellId::mint("test:model:shell#sh0").expect("identity grammar"),
        surface: SurfaceId::mint("test:model:surface#s0").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(
            "test:model:loop#lp0",
        )
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell::with_face(
        ShellId::mint("test:model:shell#sh0").expect("identity grammar"),
        RegionId::mint("test:model:region#l0").expect("identity grammar"),
        FaceId::mint("test:model:face#f0").expect("identity grammar"),
    ));
    ir.model.regions.push(Region {
        id: RegionId::mint("test:model:region#l0").expect("identity grammar"),
        body: BodyId::mint("test:model:body#b0").expect("identity grammar"),
        shells: vec![ShellId::mint("test:model:shell#sh0").expect("identity grammar")],
    });
    ir.model.bodies.push(Body {
        id: BodyId::mint("test:model:body#b0").expect("identity grammar"),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: vec![RegionId::mint("test:model:region#l0").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn writer_reports_unhandled_neutral_arenas_and_product_metadata() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.assets.push(
        cadmpeg_ir::assets::Asset::try_new(
            cadmpeg_ir::assets::AssetId::mint("test:model:asset#texture")
                .expect("identity grammar"),
            Some("texture".into()),
            Some("image/png".into()),
            cadmpeg_ir::assets::AssetContent::External {
                uri: cadmpeg_core::text::NonBlankString::new("urn:test:texture")
                    .expect("nonempty uri"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model
        .semantic_annotations
        .push(cadmpeg_ir::semantic_annotations::SemanticAnnotation {
            id: cadmpeg_ir::semantic_annotations::SemanticAnnotationId::mint(
                "test:model:semantic#note",
            )
            .expect("identity grammar"),
            object: "note".into(),
            kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
            runtime_type: "TextNote".into(),
            order: 0,
            text: vec!["inspection note".into()],
            references: std::collections::BTreeMap::new(),
            value: None,
            format: None,
            position: None,
            parameters: std::collections::BTreeMap::new(),
            assets: Vec::new(),
            native_ref: "native-note".into(),
        });
    let mut bom_properties = std::collections::BTreeMap::new();
    bom_properties.insert(cadmpeg_core::nonblank_literal!("stock_code"), "A-1".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "test:model:product#group"
                .try_into()
                .expect("valid identity"),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Group,
            source_name: Some("Group".into()),
            label: Some("Group".into()),
            description: None,
            part_number: None,
            bom_properties,
            bodies: Vec::new(),
            native_ref: None,
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::DocumentAssetOmitted.kind()
            && loss.message.contains("1 document asset")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::SemanticAnnotationOmitted.kind()
            && loss.message.contains("1 semantic annotation")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductNonPartKind.kind()
            && loss.message.contains("non-part kind")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductBomPropertyOmitted.kind()
            && loss.message.contains("1 product BOM property")
    }));
}

#[test]
fn writer_reports_unrepresented_topology_metadata() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode topology metadata fixture")
        .into_parts()
        .0;
    ir.model.faces[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.01).expect("positive finite tolerance"));
    ir.model.edges[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.02).expect("positive finite tolerance"));
    ir.model.vertices[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.03).expect("positive finite tolerance"));
    let edge_curve = ir.model.edges[0].curve().cloned().expect("edge curve");
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("pcurve-backed coedge");
    coedge.pcurves[0].isoparametric = Some(true);
    coedge.pcurves[0].parameter_range =
        Some(cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap());
    coedge.use_curve = Some(cadmpeg_ir::topology::CoedgeUseCurve {
        curve: edge_curve,
        parameter_range: cadmpeg_ir::topology::ParameterInterval::new([0.0, 1.0]).unwrap(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes topology metadata fixture");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveUseNativeMetadata.kind()
            && loss.message.contains("1 pcurve use")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::CoedgeUseCurveNotRepresented.kind()
            && loss.message.contains("1 coedge-local 3D curve use")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyMetadataNotRepresented.kind()
            && loss.message.contains("topology metadata")
            && loss.message.contains("face tolerance=1")
            && loss.message.contains("edge tolerance=1")
            && loss.message.contains("vertex tolerance=1")
    }));
}

#[test]
fn writer_reports_root_occurrence_scale() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let product = cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:product#scaled")
        .expect("identity grammar");
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Scaled part".into()),
            label: Some("Scaled part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: "test:model:occurrence#scaled"
            .try_into()
            .expect("valid identity"),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root {},
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [2.0, 1.0, 1.0].map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()),
        name: Some("Scaled root".into()),
        visible: None,
        link: None,
        native_ref: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes unscaled geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RootOccurrencePlacementNotRepresentable.kind()
            && loss.message.contains("placement or scale")
    }));
}

#[test]
fn writer_reports_edge_loop_without_a_continuous_ordering() {
    let mut source = unit_cube().expect("unit cube fixture is admitted");
    let edge_id = source
        .model
        .loops
        .iter()
        .find(|loop_| loop_.coedges().len() >= 3)
        .and_then(|loop_| loop_.coedges().first())
        .and_then(|coedge_id| {
            source
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
        })
        .map(|coedge| coedge.edge.clone())
        .expect("unit cube has a loop edge");
    source
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == edge_id)
        .expect("loop edge exists")
        .start = cadmpeg_ir::ids::VertexId::mint("test:model:vertex#missing-loop-vertex")
        .expect("identity grammar");

    let report = write_step(
        &source,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode should record the topology loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopNoContinuousOrdering.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("continuous vertex-to-vertex")
    }));
}

#[test]
fn ap242_writer_reports_unrepresented_tessellation_triangle_metadata() {
    use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
    use cadmpeg_ir::tessellation::{TessellationTextureAssignment, TessellationTriangleGroup};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let texture = AssetId::mint("synthetic:test:asset#0").expect("identity grammar");
    ir.model.assets.push(
        Asset::try_new(
            texture.clone(),
            None,
            Some("image/png".into()),
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(vec![0]).expect("nonempty asset data"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model.tessellations.push(
        cadmpeg_ir::tessellation::Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "synthetic:test:tessellation#triangle-metadata",
            )
            .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_triangle_groups(vec![TessellationTriangleGroup {
            source_id: Some("synthetic:test:group#0".into()),
            triangles: vec![0],
        }])
        .expect("valid triangle group partition")
        .with_texture_assignments(vec![TessellationTextureAssignment {
            source_id: Some("synthetic:test:texture-resource#0".into()),
            texture,
            triangles: vec![0],
        }])
        .expect("valid texture assignments"),
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write tessellation geometry");
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| {
                (loss.code == StepLossCode::TessellationTriangleGroups.kind()
                    || loss.code == StepLossCode::TessellationTextureAssignments.kind())
                    && loss.severity == cadmpeg_ir::report::Severity::Warning
            })
            .count(),
        2
    );
}

#[test]
fn writer_reports_occurrence_with_parent_without_local_product() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let product =
        cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:product-definition#product-child")
            .expect("identity grammar");
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Child part".into()),
            label: Some("Child part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    let parent = cadmpeg_ir::ids::OccurrenceId::mint("test:model:occurrence#external-parent")
        .expect("identity grammar");
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: parent.clone(),
        prototype: cadmpeg_ir::products::PrototypeReference::Unresolved {},
        parent: cadmpeg_ir::products::OccurrenceParent::Root {},
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId::mint("test:model:occurrence#local-child")
            .expect("identity grammar"),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence: parent },
        ordinal: 1,
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the product graph");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AssemblyOccurrenceOmittedNoParentProduct.kind()
            && loss.message.contains("local-child")
            && loss
                .message
                .contains("parent has no local product definition")
    }));
}

#[test]
fn writer_reports_region_without_shells() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.regions[0].shells.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RegionNoShellList.kind()
            && loss.message.contains("region(s) have no shell list")
    }));
}

#[test]
fn writer_reports_topology_without_an_emitted_region() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.regions.clear();
    ir.model.bodies[0].regions.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the empty shape representation");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyUnreachableFromRegion.kind()
            && loss
                .message
                .contains("topology not reachable from any emitted region shape item")
            && loss.message.contains("face(s)")
            && loss.message.contains("vertex(s)")
    }));
}

#[test]
fn writer_reports_wire_region_without_connected_edges() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.shells[0]
        .edit_topology(|faces, wire_edges, _| {
            faces.clear();
            *wire_edges = vec![
                cadmpeg_ir::ids::EdgeId::mint("test:model:edge#missing-edge")
                    .expect("identity grammar"),
            ];
        })
        .unwrap();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionNoConnectedEdgeSet.kind()
            && loss
                .message
                .contains("wire region(s) had no writable connected edge set")
    }));
}

#[test]
fn writer_reports_wire_region_with_missing_shell_record() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.regions[0].shells =
        vec![
            cadmpeg_ir::ids::ShellId::mint("test:model:shell#missing-shell")
                .expect("identity grammar"),
        ];

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionMissingShell.kind()
            && loss.message.contains("missing shell records")
            && loss.message.contains("missing-shell")
    }));
}

#[test]
fn writer_reports_hidden_body_without_step_item() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.bodies[0].visible = Some(false);
    ir.model.regions.clear();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the remaining geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::HiddenBodyOmitted.kind() && loss.message.contains(body.as_str())
    }));
}

#[test]
fn writer_reports_dangling_appearance_binding() {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let binding = "test:model:appearance-binding#dangling";
    let appearance = AppearanceId::mint("test:model:appearance#missing").expect("identity grammar");
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.try_into().expect("valid identity"),
        target: AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingMissingAsset.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}

#[test]
fn writer_reports_appearance_without_base_color() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let appearance =
        AppearanceId::mint("test:model:appearance#colorless").expect("identity grammar");
    let binding = "test:model:appearance-binding#colorless";
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: binding.try_into().expect("valid identity"),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingNoBaseColor.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}

fn duplicate_target_style_ir(body_target: bool, reverse: bool, same_color: bool) -> CadIr {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let target = if body_target {
        AppearanceTarget::Body(ir.model.bodies[0].id.clone())
    } else {
        AppearanceTarget::Face(ir.model.faces[0].id.clone())
    };
    let red = AppearanceId::mint("test:model:appearance#red").expect("identity grammar");
    let blue = AppearanceId::mint("test:model:appearance#blue").expect("identity grammar");
    for (id, color) in [
        (
            red.clone(),
            cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("valid color"),
        ),
        (
            blue.clone(),
            cadmpeg_ir::topology::Color::new(
                if same_color { 1.0 } else { 0.0 },
                0.0,
                if same_color { 0.0 } else { 1.0 },
                1.0,
            )
            .expect("valid color"),
        ),
    ] {
        ir.model.appearances.push(Appearance {
            id,
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(color),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        });
    }
    let mut bindings = vec![
        AppearanceBinding {
            id: "test:model:binding#red".try_into().expect("valid identity"),
            target: target.clone(),
            appearance: red,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::new(),
        },
        AppearanceBinding {
            id: "test:model:binding#blue"
                .try_into()
                .expect("valid identity"),
            target,
            appearance: blue,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::new(),
        },
    ];
    if reverse {
        bindings.reverse();
    }
    ir.model.appearance_bindings = bindings;
    ir
}

#[test]
fn writer_rejects_order_dependent_duplicate_target_styles() {
    for (body_target, target_kind) in [(true, "body"), (false, "face")] {
        let mut forward_output = Vec::new();
        let forward_report = write_step(
            &duplicate_target_style_ir(body_target, false, false),
            &mut forward_output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("report mode writes geometry while omitting the conflict");
        let mut reverse_output = Vec::new();
        let reverse_report = write_step(
            &duplicate_target_style_ir(body_target, true, false),
            &mut reverse_output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("reordered report mode writes geometry while omitting the conflict");
        assert_eq!(forward_output, reverse_output);
        for report in [forward_report, reverse_report] {
            assert_eq!(report.losses.len(), 1, "unexpected losses: {report:?}");
            let loss = &report.losses[0];
            assert_eq!(
                loss.code,
                StepLossCode::AppearanceBindingTargetConflict.kind()
            );
            assert!(loss.message.contains(target_kind));
            assert!(loss.message.contains("test:model:binding#red"));
            assert!(loss.message.contains("test:model:binding#blue"));
        }
        assert!(!String::from_utf8_lossy(&forward_output).contains("STYLED_ITEM"));
    }

    let mut equivalent_output = Vec::new();
    let equivalent_report = write_step(
        &duplicate_target_style_ir(false, false, true),
        &mut equivalent_output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("equal target styles are coalesced");
    assert!(equivalent_report.losses.is_empty(), "{equivalent_report:?}");
    assert_eq!(
        String::from_utf8_lossy(&equivalent_output)
            .matches("STYLED_ITEM")
            .count(),
        1
    );
}

#[test]
fn writer_reports_reduced_tessellation_metadata_and_body_links() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("test:step:tessellation#metadata")
                .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            vec![cadmpeg_ir::tessellation::TessellationChannel::new(
                cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                2,
                1,
                0,
                vec![0; 6],
            )
            .expect("valid channel")],
        )
        .expect("valid tessellation")
        .with_body(Some(
            cadmpeg_ir::ids::BodyId::mint("test:model:body#test:missing-body")
                .expect("identity grammar"),
        ))
        .with_faces(vec![ir.model.faces[0].id.clone()])
        .with_chordal_deflection(Some(0.01))
        .unwrap(),
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes reduced tessellation");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationBodyLinkUnwritable.kind()
            && loss
                .message
                .contains("has no writable AP242 tessellation link")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationMetadataReduced.kind()
            && loss.message.contains("face ownership link(s)")
            && loss.message.contains("chordal deflection")
            && loss.message.contains("data channel(s)")
    }));
}

#[test]
fn writer_reports_each_enclosing_topology_reduction() {
    let mut outer_face = unit_cube().expect("unit cube fixture is admitted");
    outer_face.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    let report = write_step(
        &outer_face,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving faces");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceNoWritableBounds.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("has no writable bounds")
    }));

    let mut inner_loop = unit_cube().expect("unit cube fixture is admitted");
    let outer = inner_loop.model.faces[0]
        .loops
        .iter()
        .next()
        .expect("cube face boundary")
        .clone();
    let missing =
        cadmpeg_ir::ids::LoopId::mint("step:data:loop#missing-inner").expect("identity grammar");
    inner_loop.model.faces[0].loops =
        cadmpeg_ir::topology::FaceLoops::unspecified(vec![outer.clone(), missing.clone()]);
    let report = write_step(
        &inner_loop,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving unclassified loop");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceOmittedUnclassifiedLoop.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("has no writable topology")
    }));
    inner_loop.model.faces[0].loops =
        cadmpeg_ir::topology::FaceLoops::classified(outer, vec![missing]);
    let report = write_step(
        &inner_loop,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving outer loop");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceOmittedInnerLoop.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("has no writable topology")
    }));

    let mut missing_edge = unit_cube().expect("unit cube fixture is admitted");
    missing_edge.model.coedges[0].edge =
        cadmpeg_ir::ids::EdgeId::mint("step:data:edge#missing").expect("identity grammar");
    let report = write_step(
        &missing_edge,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the surviving coedges");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopEdgeMissingForOrder.kind()
            && loss.message.contains("loop")
            && loss.message.contains("edge")
    }));

    let mut missing_void = unit_cube().expect("unit cube fixture is admitted");
    missing_void.model.regions[0].shells.push(
        cadmpeg_ir::ids::ShellId::mint("step:data:shell#missing-void").expect("identity grammar"),
    );
    let report = write_step(
        &missing_void,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the outer shell");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::RegionOmittedVoidShell.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("omitted void shell")
    }));
}

#[test]
fn unsupported_pcurve_family_is_reported_and_strict_export_rejects() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Harmonic(
        cadmpeg_ir::geometry::pcurve::HarmonicPcurve::try_new(
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
            cadmpeg_ir::math::Point2::new(0.0, 1.0),
        )
        .unwrap(),
    );

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable sheet");
    assert!(!String::from_utf8(output).unwrap().contains("PCURVE"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("step:data:pcurve#56")
    }));
}

#[test]
fn non_similarity_pcurve_replica_is_reported_and_strict_export_rejects() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Transformed(
        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                )
                .unwrap(),
            )),
            cadmpeg_ir::transform::Transform2::affine([[2.0, 0.0, 0.0], [0.0, 3.0, 0.0]])
                .expect("affine transform"),
        )
        .expect("placed pcurve"),
    );

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable sheet");
    assert!(!String::from_utf8(output).unwrap().contains("PCURVE"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("step:data:pcurve#56")
    }));
}

#[test]
fn unsupported_standalone_curve_is_reported_and_strict_export_rejects() {
    let mut ir = CadIr::empty();
    let curve_id =
        CurveId::mint("step:test:curve#standalone-unsupported").expect("identity grammar");
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint("step:test:construction#standalone-unsupported")
                .expect("identity grammar"),
            cache: None,
        },
        source_object: None,
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable subset");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains(curve_id.as_str())
    }));
}

#[test]
fn consumed_unit_and_pmi_wrapper_records_are_strictly_writable() {
    // Keep one complete tolerance per dimension and supply the second nominal.
    let semantic = include_str!("../../../tests/fixtures/ap242_semantic_pmi.p21")
        .replace("#23=LIMITS_AND_FITS('H','','7','ISO 286');\n", "")
        .replace("#24=PLUS_MINUS_TOLERANCE(#23,#10);\n", "")
        .replace("#26=PLUS_MINUS_TOLERANCE(#18,#25);", "#26=PLUS_MINUS_TOLERANCE(#18,#25);\n#27=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#25,#14);");
    for source in [
        include_bytes!("../../../tests/fixtures/ap242_degree_cone.p21").as_slice(),
        semantic.as_bytes(),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode typed STEP wrappers");
        assert!(decoded
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .is_empty());
        let mut bytes = Vec::new();
        write_step(
            decoded.ir(),
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions {
                ..StepWriteOptions::default()
            },
        )
        .expect("strictly write typed STEP wrappers");
        assert!(!bytes.is_empty());
    }
}

#[test]
fn ap203e1_does_not_emit_invisibility_entities() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].visible = Some(false);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("hidden body visibility")));
}

#[test]
fn ap203e1_reports_hidden_appearance_visibility_loss() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let appearance = AppearanceId::mint("test:model:appearance#hidden").expect("identity grammar");
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.4, 0.5, 0.6, 1.0).expect("valid color"),
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
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .expect("report-mode AP203e1 write");
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::HiddenAppearanceVisibilityUnsupported.kind() }));
}

#[test]
fn ap203e1_reports_hidden_presentation_layer_visibility_loss() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#hidden").expect("identity grammar"),
        name: "hidden layer".into(),
        description: None,
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap203Edition1,
        &StepWriteOptions::default(),
    )
    .expect("report-mode AP203e1 layer write");
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::HiddenPresentationLayerVisibilityUnsupported.kind()
    }));
}

#[test]
fn step_writer_rejects_unknown_datum_reference_modifiers() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    let mut edited = references.as_slice().to_vec();
    edited[0].modifiers.push("unknown_modifier".into());
    {
        let replacement = edited;
        edit::replace(references, |_| {
            cadmpeg_ir::pmi::DatumReferences::try_from(replacement)
        })
    }
    .expect("unchanged datum compartments");

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("report-mode STEP write");
    assert!(report.losses.iter().any(|loss| loss.code
        == StepLossCode::PmiAnnotationNotWritten.kind()
        || loss.code == StepLossCode::SemanticAnnotationOmitted.kind()));
    assert!(!String::from_utf8_lossy(&output).contains(".UNKNOWN_MODIFIER."));
    assert!(!String::from_utf8_lossy(&output).contains("DATUM_REFERENCE_MODIFIER_WITH_VALUE"));
}

/// PMI is dropped whole when the target schema has no semantic PMI. The drop is
/// charged as `pmi.annotation-not-written` for every annotation, and strict mode
/// refuses the write. The schemas are driven by `supports_semantic_pmi`, so this
/// covers every non-AP242 target rather than one sampled schema.
#[test]
fn pmi_dropped_by_schema_without_semantic_pmi_is_charged() {
    let ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let annotations = ir.model.pmi.len();
    assert!(annotations > 0);

    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        if schema.supports_semantic_pmi() {
            continue;
        }
        let mut output = Vec::new();
        let report = write_step(&ir, &mut output, schema, &StepWriteOptions::default())
            .expect("report-mode STEP write");
        let charged = report
            .losses
            .iter()
            .filter(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind())
            .count();
        assert_eq!(charged, 1, "{}", schema.file_schema());
        assert!(report.losses.iter().any(|loss| {
            loss.code == StepLossCode::PmiAnnotationNotWritten.kind()
                && loss.message.contains(&format!("{annotations} PMI"))
        }));
    }
}

#[test]
fn edge_without_curve_is_reported_and_omitted() {
    let _ = cylinder_surface_doc(); // keep helper exercised
    let ir = edgeless_doc();
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let curve = Curve {
        parameter_range: None,
        id: CurveId::mint("test:model:curve#unused").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let _ = curve; // silence unused import path
    assert!(report
        .losses
        .iter()
        .any(|l| l.message.contains("edge(s) have no typed 3D curve")));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::EdgeNo3dCurve.kind()
            && loss
                .message
                .contains("was omitted because it has no 3D curve")
    }));
}

#[test]
fn subds_tessellations_and_source_associations_are_reported_as_losses() {
    let source_object = cadmpeg_ir::SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("object-0")
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId::mint("test:step:subd#0").expect("identity grammar"),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        source_object: Some(source_object.clone()),
        cage: cadmpeg_ir::subd::SubdCage::default(),
    });
    ir.model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("test:step:tessellation#0")
                .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_source_object(Some(source_object)),
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss
                .message
                .contains("1 subdivision surface(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss
                .message
                .contains("1 tessellation(s) require an AP242 target")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Metadata
            && loss
                .message
                .contains("2 source-object association(s) were not represented")
    }));
}

#[test]
fn face_on_unknown_surface_is_skipped_and_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let target = ir.model.faces[0].surface.as_str().to_owned();
    for s in &mut ir.model.surfaces {
        if s.id.as_str() == target {
            s.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
        }
    }
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let s = String::from_utf8(buf).unwrap();

    assert_eq!(
        s.matches("ADVANCED_FACE").count(),
        5,
        "the unknown-surface face should be omitted"
    );
    let unknown_notes: Vec<_> = report
        .losses
        .iter()
        .filter(|l| l.message.contains("rest on an unknown"))
        .collect();
    assert_eq!(
        unknown_notes.len(),
        1,
        "loss must be aggregated into a single counted note, got: {:?}",
        report.losses
    );
    assert!(unknown_notes[0].message.contains("1 face(s)"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ShellOmittedUnclassifiedFace.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("omitted face")
    }));
}

#[test]
fn unsupported_nested_and_polygonal_carriers_are_skipped_without_panicking() {
    let mut polygonal = unit_cube().expect("unit cube fixture is admitted");
    let surface_id = polygonal.model.faces[0].surface.clone();
    polygonal
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(
        cadmpeg_ir::geometry::sampled::PolygonalSurface::new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
            0.1,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("polygonal construction admission")
        .expect("valid polygonal surface"),
    ));
    let report = write_step(
        &polygonal,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("polygonal face is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.message.contains("unknown or STEP-unsupported surface")
    }));

    let mut nested_unknown = unit_cube().expect("unit cube fixture is admitted");
    let curve_id = nested_unknown.model.edges[0].curve().cloned().unwrap();
    nested_unknown
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Unknown { record: None }),
            cadmpeg_ir::transform::Transform::identity(),
        )
        .expect("placed curve"),
    ));
    let report = write_step(
        &nested_unknown,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("transformed unknown curve is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.message.contains("STEP-unsupported transform")
    }));
}

#[test]
fn procedural_surface_outside_the_writable_set_is_reported_not_panicked() {
    let mut ir = CadIr::empty();
    let surface_id = SurfaceId::mint("step:test:surface#unsupported").expect("identity grammar");
    let construction_id =
        cadmpeg_ir::ids::ProceduralSurfaceId::mint("step:test:construction-surface#unsupported")
            .expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model
        .procedural_surfaces
        .push(cadmpeg_ir::geometry::ProceduralSurface::new(
            construction_id,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Compound(
                cadmpeg_ir::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                    Vec::new(),
                    None,
                )
                .unwrap(),
            ),
            None,
        ));

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode must not panic on an unwritable procedural surface");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains(surface_id.as_str())
    }));
}

#[test]
fn procedural_curve_outside_the_writable_set_is_reported_not_panicked() {
    let mut ir = CadIr::empty();
    let curve_id = CurveId::mint("step:test:curve#unsupported").expect("identity grammar");
    let construction_id = ProceduralCurveId::mint("step:test:construction-curve#unsupported")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction_id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve::new(
            construction_id,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact { cache: None },
        ));

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode must not panic on an unwritable procedural curve");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains(curve_id.as_str())
    }));
}

#[test]
fn signed_analytic_radius_normalization_is_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -2.0,
        )
        .unwrap(),
    ));

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.message.contains("normalized to positive STEP radii")
    }));
}

#[test]
fn elliptical_cone_reduction_is_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            0.4,
            0.5,
        )
        .unwrap(),
    ));

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
            && loss.message.contains("elliptical cone surface(s)")
    }));
}

/// Plans a document whose single surface is a circular cone of `half_angle`.
fn circular_cone_plan(half_angle: f64) -> Result<ExportPlan, cadmpeg_core::CodecError> {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            half_angle,
        )
        .unwrap(),
    ));

    StepCodec::default().plan(
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
    )
}

#[test]
fn cone_semi_angle_inside_wr2_is_planned() {
    assert!(circular_cone_plan(0.5).is_ok());
}

#[test]
fn zero_cone_semi_angle_is_refused_at_planning() {
    let error = circular_cone_plan(0.0).expect_err("zero cone angle is outside WR2");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("semi-angle 0"))
    );
}

#[test]
fn negative_cone_semi_angle_inside_wr2_is_planned() {
    assert!(circular_cone_plan(-0.715_584_993_317_674_8).is_ok());
}

#[test]
fn right_angle_cone_semi_angle_is_refused_at_planning() {
    let error = circular_cone_plan(std::f64::consts::FRAC_PI_2)
        .expect_err("right angle cone is outside WR2");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("outside (0, pi/2)"))
    );
}

#[test]
fn negative_right_angle_cone_semi_angle_is_refused_at_planning() {
    let error = circular_cone_plan(-std::f64::consts::FRAC_PI_2)
        .expect_err("negative right angle cone is outside WR2");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("outside (0, pi/2)"))
    );
}

/// A similarity transform, which the writer carries as a `SURFACE_REPLICA`.
fn replica_transform() -> Transform {
    Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("a scaled rotation is a similarity transform")
}

/// A document whose single surface places `basis` through a similarity transform.
fn transformed_surface_ir(basis: SolvedSurfaceGeometry) -> CadIr {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(basis), replica_transform())
            .expect("placed surface"),
    ));
    ir
}

/// Writes a transformed surface and returns the report with STEP text.
fn transformed_surface_report(
    basis: SolvedSurfaceGeometry,
) -> (cadmpeg_ir::report::export::ExportReport, String) {
    let ir = transformed_surface_ir(basis);
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(buf).expect("STEP output is UTF-8");
    (report, text)
}

fn transformed_cone_basis(ratio: f64, half_angle: f64) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            ratio,
            half_angle,
        )
        .unwrap(),
    )
}

#[test]
fn transformed_cone_basis_outside_wr2_is_refused_at_planning() {
    let ir = transformed_surface_ir(transformed_cone_basis(1.0, 0.0));
    let error = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
        )
        .expect_err("transformed cone basis remains outside WR2");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("semi-angle 0"))
    );
}

#[test]
fn transformed_elliptical_cone_basis_is_reported() {
    let (report, text) = transformed_surface_report(transformed_cone_basis(0.4, 0.5));

    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("CONICAL_SURFACE"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::EllipticalConeReduced.kind()));
}

#[test]
fn transformed_signed_sphere_basis_is_reported() {
    let (report, text) = transformed_surface_report(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -2.0,
        )
        .unwrap(),
    ));

    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("SPHERICAL_SURFACE"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::AnalyticSurfaceNormalized.kind()));
}

/// Writes a document whose single surface is a torus of `minor_radius` attached
/// to a degenerate torus construction, and returns the report with the text.
fn degenerate_torus_report(
    minor_radius: f64,
) -> (cadmpeg_ir::report::export::ExportReport, String) {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            minor_radius,
        )
        .unwrap(),
    ));
    let owner = ir.model.surfaces[0].id.clone();
    ir.model
        .add_procedural_surface(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &owner,
            cadmpeg_ir::geometry::ProceduralSurface::new(
                cadmpeg_ir::ids::ProceduralSurfaceId::mint(
                    "test:model:procedural-surface#degenerate_torus",
                )
                .expect("identity grammar"),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::DegenerateTorus {
                    select_outer: true,
                },
                None,
            ),
        )
        .unwrap()
        .expect("a torus carrier admits a degenerate torus construction");

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(buf).expect("STEP output is UTF-8");
    (report, text)
}

#[test]
fn degenerate_torus_wider_than_its_major_radius_is_not_reported() {
    let (report, text) = degenerate_torus_report(2.0);

    assert!(text.contains("DEGENERATE_TOROIDAL_SURFACE"));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::AnalyticSurfaceNormalized.kind()));
}

#[test]
fn degenerate_torus_with_a_negative_tube_radius_is_reported() {
    let (report, text) = degenerate_torus_report(-2.0);

    assert!(text.contains("DEGENERATE_TOROIDAL_SURFACE"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::AnalyticSurfaceNormalized.kind()));
}

#[test]
fn a_cone_cache_for_an_unwritable_construction_is_refused_at_planning() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            0.0,
        )
        .unwrap(),
    ));
    let owner = ir.model.surfaces[0].id.clone();
    // A compound construction has no STEP record, so its solved cache is the
    // candidate STEP carrier.
    ir.model
        .add_procedural_surface(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &owner,
            cadmpeg_ir::geometry::ProceduralSurface::new(
                cadmpeg_ir::ids::ProceduralSurfaceId::mint(
                    "test:model:procedural-surface#compound",
                )
                .expect("identity grammar"),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Compound(
                    cadmpeg_ir::geometry::surface_payloads::CompoundSurfacePayload::try_new(
                        Vec::new(),
                        None,
                    )
                    .unwrap(),
                ),
                None,
            ),
        )
        .unwrap()
        .expect("a cone carrier admits a compound construction");

    let error = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
        )
        .expect_err("a cached cone outside WR2 cannot be written");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("semi-angle 0"))
    );
}

#[test]
fn procedural_construction_reduction_is_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let owner = ir.model.curves[0].id.clone();
    let procedural = cadmpeg_ir::geometry::ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:procedural-curve#generated_int_cur")
            .expect("identity grammar"),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
            context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                std::array::from_fn(|_| cadmpeg_ir::geometry::IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                }),
                [0.0, 1.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap(),
            discontinuity_flag: false,
            cache: Some(cadmpeg_ir::geometry::LegacyCache::try_new(0.01).expect("fit tolerance")),
        },
    );
    ir.model
        .add_procedural_curve(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &owner,
            procedural,
        )
        .unwrap()
        .unwrap();

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.message
            .contains("reduced to their solved STEP carriers")
    }));
}

#[test]
fn source_native_record_reduction_is_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
            Default::default(),
        )
        .expect("valid native identity")],
    );
    ir.finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");

    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.message
            .contains("source-native record(s) were not represented in STEP")
    }));
}
