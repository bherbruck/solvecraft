// SPDX-License-Identifier: Apache-2.0
//! IR → STEP Part 21 builder.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::appearance::{Appearance, AppearanceTarget};
#[cfg(test)]
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    analytic::TorusSurface, pcurve::Pcurve, Curve, CurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface,
};
use cadmpeg_ir::ids::{AppearanceBindingId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::pmi::{
    DatumTargetForm, DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiDefinition,
    PmiQuantity, PmiTarget,
};
use cadmpeg_ir::presentation::PresentationItem;
use cadmpeg_ir::products::{AssemblyGraph, OccurrenceParent, PrototypeReference};
#[cfg(test)]
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

use crate::geometry;
use crate::loss::StepLossCode;
use crate::options::{StepSchema, StepWriteOptions};
use crate::writer::{refs, string, Emitter, Ref};

mod ownership;
mod wire;

const EPS_IDENTITY: f64 = 1.0e-12;

/// Serializes an IR document as an ISO 10303-21 STEP Part 21 file declaring
/// `schema`.
///
/// The output declares that schema and a millimetre length unit. Coordinate
/// values are not rescaled. The IR linear tolerance becomes the representation
/// context's uncertainty value.
///
/// Geometry conversion completes before this function writes the header. The
/// function streams the header, DATA instances, and closing records to `w`. An
/// I/O error can therefore leave a partial file and returns no report.
///
/// On success, the report contains DATA entity counts and loss notes for
/// reductions that the selected schema cannot carry.
#[cfg(test)]
pub(crate) fn write_step(
    ir: &CadIr,
    w: &mut impl Write,
    schema: StepSchema,
    opts: &StepWriteOptions,
) -> std::io::Result<ExportReport> {
    let plan = crate::StepCodec {
        options: opts.clone(),
    }
    .plan(
        EncodeInput::new(ir, None),
        TargetRequest::Explicit(schema.descriptor().id.as_str()),
    )
    .map_err(std::io::Error::other)?;
    plan.write_to(w).map_err(std::io::Error::other)
}

/// Report components produced by the STEP writer before the caller fixes its
/// fidelity and source-target context.
pub(crate) struct StepWriteOutcome {
    pub(crate) census: cadmpeg_ir::report::export::EntityCensus,
    pub(crate) losses: Vec<LossNote>,
    pub(crate) notes: Vec<String>,
}

/// Writes STEP bytes and returns the facts measured by the writer.
///
/// A real the writer computes that is not finite, and a replica transform
/// that is not a similarity, refuse the file before any byte is written:
/// Part 21 states no such number, and its transformation operator states
/// only a similarity.
pub(crate) fn write_step_outcome(
    ir: &CadIr,
    w: &mut (impl Write + ?Sized),
    schema: StepSchema,
    opts: &StepWriteOptions,
) -> Result<StepWriteOutcome, cadmpeg_core::CodecError> {
    let definitions = ir
        .model
        .product_definitions
        .iter()
        .map(|product| &product.id)
        .collect::<BTreeSet<_>>();
    let missing_definitions = ir.model.occurrences.iter().filter(|occurrence| {
        matches!(&occurrence.prototype, PrototypeReference::Local { definition } if !definitions.contains(definition))
    }).count();
    if missing_definitions != 0 {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "STEP cannot write {missing_definitions} occurrence record(s) whose local product definition is missing"
        )));
    }
    for surface in &ir.model.surfaces {
        if let Some(half_angle) = surface
            .geometry
            .solved()
            .and_then(out_of_domain_cone_half_angle)
        {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "STEP conical_surface cannot represent surface '{}' with semi-angle {half_angle} outside (0, pi/2)",
                surface.id
            )));
        }
    }
    let mut b = Builder::new(ir, schema)?;
    b.build();
    let outcome = b.finish_outcome();
    let lines = b.emitter.into_lines()?;

    write_header(w, schema, opts)?;
    writeln!(w, "DATA;")?;
    for line in &lines {
        writeln!(w, "{line}")?;
    }
    writeln!(w, "ENDSEC;")?;
    writeln!(w, "END-ISO-10303-21;")?;
    Ok(outcome)
}

fn out_of_domain_cone_half_angle(mut geometry: &SolvedSurfaceGeometry) -> Option<f64> {
    loop {
        match geometry {
            SolvedSurfaceGeometry::Cone(cone) => {
                let angle = cone.half_angle().get();
                // Semi-angle 0 is a cylinder; basis export maps it to CYLINDRICAL_SURFACE.
                return (angle.abs() >= std::f64::consts::FRAC_PI_2).then_some(angle);
            }
            SolvedSurfaceGeometry::Transformed(placed) => geometry = placed.basis(),
            _ => return None,
        }
    }
}

fn step_triangle_index(index: u32) -> u64 {
    u64::from(index) + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn triangle_index_widens_before_one_based_conversion() {
        assert_eq!(super::step_triangle_index(0), 1);
        assert_eq!(super::step_triangle_index(u32::MAX), 4_294_967_296);
    }
}

fn write_header(
    w: &mut (impl Write + ?Sized),
    schema: StepSchema,
    opts: &StepWriteOptions,
) -> std::io::Result<()> {
    let ts = opts.timestamp.as_deref().unwrap_or("1970-01-01T00:00:00");
    writeln!(w, "ISO-10303-21;")?;
    writeln!(w, "HEADER;")?;
    writeln!(
        w,
        "FILE_DESCRIPTION(({}),'2;1');",
        string("CAD model exported by cadmpeg")
    )?;
    writeln!(
        w,
        "FILE_NAME({},{},({}),({}),{},{},{});",
        string(&opts.product_name),
        string(ts),
        string(&opts.author),
        string(&opts.organization),
        string("cadmpeg-step"),
        string(&opts.originating_system),
        string("")
    )?;
    writeln!(w, "FILE_SCHEMA(({}));", string(schema.file_schema()))?;
    writeln!(w, "ENDSEC;")?;
    Ok(())
}

#[derive(Clone, Copy)]
struct ColorSpec<'a> {
    color: cadmpeg_ir::topology::Color,
    appearance: Option<&'a Appearance>,
    hidden: bool,
}

fn equivalent_style_spec(left: ColorSpec<'_>, right: ColorSpec<'_>) -> bool {
    left.color == right.color
        && left.hidden == right.hidden
        && left
            .appearance
            .and_then(|appearance| appearance.name.as_deref())
            == right
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
}

struct LoopSegment {
    coedge_id: String,
    start_vertex: String,
    end_vertex: String,
}

/// An analytic surface record in the emitted file, held as the IR carrier whose
/// values that record carries. The export census counts these, so it states
/// what the file holds and not what the document holds.
enum WrittenAnalyticSurface<'a> {
    /// Written by `geometry::surface`, which takes a sphere radius and a torus
    /// tube radius through `abs()` and reverses a negative cone's axis. A
    /// `Transformed` carrier is held here through its emitted basis.
    Carrier(&'a SolvedSurfaceGeometry),
    /// Written as `DEGENERATE_TOROIDAL_SURFACE`, which represents a tube radius
    /// larger than the major radius and takes both radii through `abs()`.
    DegenerateTorus(&'a TorusSurface),
}

#[derive(Clone, Copy)]
struct WrittenSurface {
    reference: Ref,
    reversed_chart: bool,
}

pub(crate) struct Builder<'a> {
    ir: &'a CadIr,
    schema: StepSchema,
    emitter: Emitter,
    losses: Vec<LossNote>,

    points: HashMap<&'a str, &'a Point>,
    bodies: HashMap<&'a str, &'a Body>,
    shells: HashMap<&'a str, &'a Shell>,
    faces: HashMap<&'a str, &'a Face>,
    loops: HashMap<&'a str, &'a Loop>,
    vertices: HashMap<&'a str, &'a Vertex>,
    edges: HashMap<&'a str, &'a Edge>,
    coedges: HashMap<&'a str, &'a Coedge>,
    surfaces: HashMap<&'a str, &'a Surface>,
    curves: HashMap<&'a str, &'a Curve>,
    pcurves: HashMap<&'a str, &'a Pcurve>,
    procedural_surfaces: HashMap<&'a str, &'a ProceduralSurface>,
    procedural_curves: HashMap<&'a str, &'a ProceduralCurve>,
    edge_coedges: HashMap<&'a str, Vec<(&'a str, &'a str, &'a str)>>,

    surface_refs: HashMap<String, WrittenSurface>,
    curve_refs: HashMap<String, Ref>,
    edge_refs: HashMap<String, Ref>,
    vertex_refs: HashMap<String, Ref>,
    point_refs: HashMap<String, Ref>,
    pcurve_context: Option<Ref>,
    active_surfaces: BTreeSet<String>,
    pub(crate) active_curves: BTreeSet<String>,
    written_procedural_surfaces: BTreeSet<String>,
    written_procedural_curves: BTreeSet<String>,
    /// Analytic surface records the writer put in the file, in emission order.
    written_analytic_surfaces: Vec<WrittenAnalyticSurface<'a>>,

    /// Edges skipped because they carry no attributed 3D curve, deduplicated
    /// (a shared edge is reached once per coedge) and aggregated into a single
    /// counted loss note.
    curveless_edges: BTreeSet<String>,

    /// Faces skipped because their surface geometry is unknown (opaque), so no
    /// STEP surface exists to build an `ADVANCED_FACE` on. Deduplicated (a face
    /// is reached once per shell) and aggregated into a single counted loss.
    unknown_surface_faces: BTreeSet<String>,

    /// Individual enclosing topology relations that could not be emitted.
    topology_relation_losses: BTreeSet<String>,

    face_step_refs: HashMap<String, Ref>,
    /// First emitted exact solid or shell for each body, used by AP242 tessellation links.
    body_step_refs: HashMap<String, Ref>,
    default_product_definition_shape: Option<Ref>,
    body_shape_refs: HashMap<String, Ref>,
    pub(crate) body_item_refs: HashMap<String, Vec<Ref>>,
    body_step_item_refs: HashMap<String, Vec<Ref>>,
    product_step_refs: HashMap<String, Ref>,
    occurrence_step_refs: HashMap<String, Ref>,
    tessellation_step_refs: HashMap<String, Ref>,
    pmi_step_refs: HashMap<String, Ref>,
    written_appearance_bindings: BTreeSet<AppearanceBindingId>,
    conflicted_appearance_bindings: BTreeSet<AppearanceBindingId>,
    appearance_binding_target_conflicts: BTreeMap<(String, String), BTreeSet<AppearanceBindingId>>,
    hidden_appearance_items: Vec<Ref>,
    hidden_presentation_layer_items: Vec<Ref>,
    unstyled_colors: usize,
    unwritten_geometry_carriers: BTreeSet<String>,
    unwritten_pcurve_carriers: BTreeSet<String>,
    missing_parent_products: BTreeSet<String>,
    empty_regions: BTreeSet<String>,
    empty_wire_regions: BTreeSet<String>,
    missing_wire_shells: BTreeSet<(String, String)>,
    hidden_bodies_without_items: BTreeSet<String>,
    hidden_presentation_layers_without_items: BTreeSet<String>,
    dangling_appearance_bindings: BTreeSet<(AppearanceBindingId, String)>,
    colorless_appearance_bindings: BTreeSet<(AppearanceBindingId, String)>,
    written_pmi: usize,
    length_unit: Option<Ref>,
    angle_unit: Option<Ref>,
    ratio_unit: Option<Ref>,
    geometry_emission_depth: usize,
    construction_supports: BTreeSet<String>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(ir: &'a CadIr, schema: StepSchema) -> Result<Self, cadmpeg_core::CodecError> {
        let construction_supports = ownership::construction_supports(ir)?;
        let loop_faces = ir
            .model
            .faces
            .iter()
            .flat_map(|face| {
                face.loops
                    .iter()
                    .map(move |loop_id| (loop_id.as_str(), face))
            })
            .collect::<HashMap<_, _>>();
        let coedge_faces: HashMap<&str, (&str, &str)> =
            ir.model
                .loops
                .iter()
                .filter_map(|loop_| loop_faces.get(loop_.id.as_str()).map(|face| (loop_, *face)))
                .flat_map(|(loop_, face)| {
                    loop_.coedges().iter().map(move |coedge| {
                        (coedge.as_str(), (face.surface.as_str(), face.id.as_str()))
                    })
                })
                .collect();
        let mut edge_coedges = HashMap::<&str, Vec<(&str, &str, &str)>>::new();
        for coedge in &ir.model.coedges {
            let Some((surface, face)) = coedge_faces.get(coedge.id.as_str()) else {
                continue;
            };
            for pcurve in &coedge.pcurves {
                edge_coedges.entry(coedge.edge.as_str()).or_default().push((
                    pcurve.pcurve.as_str(),
                    *surface,
                    *face,
                ));
            }
        }
        Ok(Builder {
            ir,
            schema,
            emitter: Emitter::new(),
            losses: Vec::new(),
            points: ir.model.points.iter().map(|p| (p.id.as_str(), p)).collect(),
            bodies: ir
                .model
                .bodies
                .iter()
                .map(|body| (body.id.as_str(), body))
                .collect(),
            shells: ir
                .model
                .shells
                .iter()
                .map(|shell| (shell.id.as_str(), shell))
                .collect(),
            faces: ir
                .model
                .faces
                .iter()
                .map(|face| (face.id.as_str(), face))
                .collect(),
            loops: ir
                .model
                .loops
                .iter()
                .map(|loop_| (loop_.id.as_str(), loop_))
                .collect(),
            vertices: ir
                .model
                .vertices
                .iter()
                .map(|v| (v.id.as_str(), v))
                .collect(),
            edges: ir.model.edges.iter().map(|e| (e.id.as_str(), e)).collect(),
            coedges: ir
                .model
                .coedges
                .iter()
                .map(|c| (c.id.as_str(), c))
                .collect(),
            surfaces: ir
                .model
                .surfaces
                .iter()
                .map(|s| (s.id.as_str(), s))
                .collect(),
            curves: ir.model.curves.iter().map(|c| (c.id.as_str(), c)).collect(),
            pcurves: ir
                .model
                .pcurves
                .iter()
                .map(|p| (p.id.as_str(), p))
                .collect(),
            procedural_surfaces: ir
                .model
                .procedural_surfaces
                .iter()
                .filter_map(|surface| {
                    Some((
                        ir.model.procedural_surface_owner(&surface.id)?.as_str(),
                        surface,
                    ))
                })
                .collect(),
            procedural_curves: ir
                .model
                .procedural_curves
                .iter()
                .filter_map(|curve| {
                    Some((ir.model.procedural_curve_owner(&curve.id)?.as_str(), curve))
                })
                .collect(),
            edge_coedges,
            surface_refs: HashMap::new(),
            curve_refs: HashMap::new(),
            edge_refs: HashMap::new(),
            vertex_refs: HashMap::new(),
            point_refs: HashMap::new(),
            pcurve_context: None,
            active_surfaces: BTreeSet::new(),
            active_curves: BTreeSet::new(),
            written_procedural_surfaces: BTreeSet::new(),
            written_procedural_curves: BTreeSet::new(),
            written_analytic_surfaces: Vec::new(),
            curveless_edges: BTreeSet::new(),
            unknown_surface_faces: BTreeSet::new(),
            topology_relation_losses: BTreeSet::new(),
            face_step_refs: HashMap::new(),
            body_step_refs: HashMap::new(),
            default_product_definition_shape: None,
            body_shape_refs: HashMap::new(),
            body_item_refs: HashMap::new(),
            body_step_item_refs: HashMap::new(),
            product_step_refs: HashMap::new(),
            occurrence_step_refs: HashMap::new(),
            tessellation_step_refs: HashMap::new(),
            pmi_step_refs: HashMap::new(),
            written_appearance_bindings: BTreeSet::new(),
            conflicted_appearance_bindings: BTreeSet::new(),
            appearance_binding_target_conflicts: BTreeMap::new(),
            hidden_appearance_items: Vec::new(),
            hidden_presentation_layer_items: Vec::new(),
            unstyled_colors: 0,
            unwritten_geometry_carriers: BTreeSet::new(),
            unwritten_pcurve_carriers: BTreeSet::new(),
            missing_parent_products: BTreeSet::new(),
            empty_regions: BTreeSet::new(),
            empty_wire_regions: BTreeSet::new(),
            missing_wire_shells: BTreeSet::new(),
            hidden_bodies_without_items: BTreeSet::new(),
            hidden_presentation_layers_without_items: BTreeSet::new(),
            dangling_appearance_bindings: BTreeSet::new(),
            colorless_appearance_bindings: BTreeSet::new(),
            written_pmi: 0,
            length_unit: None,
            angle_unit: None,
            ratio_unit: None,
            geometry_emission_depth: 0,
            construction_supports,
        })
    }

    fn loss(&mut self, code: StepLossCode, message: String) {
        self.losses.push(code.note(message));
    }

    fn topology_relation_loss(&mut self, identity: String, code: StepLossCode, message: String) {
        if self.topology_relation_losses.insert(identity) {
            self.loss(code, message);
        }
    }

    pub(crate) fn build(&mut self) {
        if !self.ir.model.product_definitions.is_empty() {
            let owned = self
                .ir
                .model
                .product_definitions
                .iter()
                .flat_map(|product| &product.bodies)
                .collect::<BTreeSet<_>>();
            let count = self
                .ir
                .model
                .bodies
                .iter()
                .filter(|body| !owned.contains(&body.id))
                .count();
            if count != 0 {
                self.loss(StepLossCode::BodyWithoutProductRepresentation, format!(
                    "{count} body record(s) have no product representation; their shape items are not attached to a representation"
                ));
            }
        }
        let surface_regions = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| matches!(body.kind, BodyKind::Solid | BodyKind::Sheet))
            .flat_map(|body| &body.regions)
            .collect::<BTreeSet<_>>();
        let mut wire_edges = BTreeSet::new();
        let mut free_vertices = BTreeSet::new();
        for shell in &self.ir.model.shells {
            if surface_regions.contains(&shell.region) {
                wire_edges.extend(shell.wire_edges());
                free_vertices.extend(shell.free_vertices());
            }
        }
        if !wire_edges.is_empty() || !free_vertices.is_empty() {
            self.loss(StepLossCode::SurfaceShellWireTopologyOmitted, format!(
                "{} wire edge record(s) and {} free vertex record(s) in solid or sheet shells are not written",
                wire_edges.len(), free_vertices.len()
            ));
        }
        let context = self.emit_context();

        let shape_items = self.emit_shape_items(context);
        let mut standalone_items = self.emit_standalone_geometry();
        let has_standalone_geometry = !standalone_items.is_empty();
        let mut emitted_items = shape_items;
        emitted_items.extend(standalone_items.iter().copied());
        if emitted_items.is_empty() && !self.ir.model.bodies.is_empty() {
            self.loss(
                StepLossCode::NoExportableSolids,
                "no exportable solids: the IR document contains no body/region/shell \
                          geometry, so the STEP representation is empty"
                    .to_string(),
            );
            emitted_items.clear();
        }
        let mut items = emitted_items;
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        );
        items.push(origin);

        if self.ir.model.product_definitions.is_empty() {
            let product_def_shape = self.emit_product_structure();
            self.default_product_definition_shape = Some(product_def_shape);
            let representation_kind = if !has_standalone_geometry
                && !self.ir.model.bodies.is_empty()
                && self.ir.model.bodies.iter().all(|body| {
                    body.transform
                        .is_none_or(|transform| is_identity(&transform.rows()))
                })
                && self
                    .ir
                    .model
                    .bodies
                    .iter()
                    .all(|body| body.kind == BodyKind::Solid)
            {
                "ADVANCED_BREP_SHAPE_REPRESENTATION"
            } else {
                "SHAPE_REPRESENTATION"
            };
            let representation = self.emitter.emit(
                representation_kind,
                &format!("'',{},{context}", refs(&items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{product_def_shape},{representation}"),
            );
        } else {
            self.emit_product_graph(context);
            if has_standalone_geometry {
                standalone_items.push(origin);
                self.emitter.emit(
                    "SHAPE_REPRESENTATION",
                    &format!(
                        "{},{},{context}",
                        string("standalone geometry"),
                        refs(&standalone_items)
                    ),
                );
            }
        }

        self.emit_visibility();
        self.emit_tessellations(context);
        self.emit_presentation(context);
        self.emit_appearance_visibility();
        self.emit_pmi(context);
        self.emit_layers();
        self.emit_layer_visibility();
        self.note_unrepresented();
    }

    fn emit_presentation(&mut self, context: Ref) {
        enum StyleKind {
            Surface,
            Curve,
            Point,
        }

        let ir = self.ir;
        let appearances: HashMap<&str, &Appearance> = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance))
            .collect();
        let hidden_binding_ids = ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.visible == Some(false))
            .map(|binding| binding.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut body_candidates: HashMap<&str, Vec<(ColorSpec<'_>, &AppearanceBindingId)>> =
            HashMap::new();
        let mut face_candidates: HashMap<&str, Vec<(ColorSpec<'_>, &AppearanceBindingId)>> =
            HashMap::new();
        let mut dangling_appearance_bindings = BTreeSet::new();
        let mut colorless_appearance_bindings = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                dangling_appearance_bindings
                    .insert((binding.id.clone(), binding.appearance.as_str().to_owned()));
                continue;
            };
            let Some(color) = appearance.base_color else {
                colorless_appearance_bindings
                    .insert((binding.id.clone(), binding.appearance.as_str().to_owned()));
                continue;
            };
            let spec = ColorSpec {
                color,
                appearance: Some(appearance),
                hidden: hidden_binding_ids.contains(binding.id.as_str()),
            };
            match &binding.target {
                AppearanceTarget::Body(id) => {
                    body_candidates
                        .entry(id.as_str())
                        .or_default()
                        .push((spec, &binding.id));
                }
                AppearanceTarget::Face(id) => {
                    face_candidates
                        .entry(id.as_str())
                        .or_default()
                        .push((spec, &binding.id));
                }
                AppearanceTarget::Surface(_)
                | AppearanceTarget::Curve(_)
                | AppearanceTarget::Point(_)
                | AppearanceTarget::Edge(_)
                | AppearanceTarget::Vertex(_)
                | AppearanceTarget::Tessellation(_)
                | AppearanceTarget::Source { .. } => {}
            }
        }
        self.dangling_appearance_bindings
            .extend(dangling_appearance_bindings);
        self.colorless_appearance_bindings
            .extend(colorless_appearance_bindings);

        let mut body_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut face_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut conflicting_body_targets = BTreeSet::new();
        let mut conflicting_face_targets = BTreeSet::new();
        let mut conflicted_binding_ids = BTreeSet::new();
        let mut target_conflicts = BTreeMap::new();
        for (&target, candidates) in &body_candidates {
            let Some((first, _)) = candidates.first().copied() else {
                continue;
            };
            if candidates
                .iter()
                .copied()
                .any(|(candidate, _)| !equivalent_style_spec(first, candidate))
            {
                conflicting_body_targets.insert(target.to_string());
                let ids = candidates
                    .iter()
                    .map(|(_, id)| (*id).clone())
                    .collect::<BTreeSet<_>>();
                conflicted_binding_ids.extend(ids.iter().cloned());
                target_conflicts.insert(("body".into(), target.into()), ids);
            } else {
                body_colors.insert(target, first);
            }
        }
        for (&target, candidates) in &face_candidates {
            let Some((first, _)) = candidates.first().copied() else {
                continue;
            };
            if candidates
                .iter()
                .copied()
                .any(|(candidate, _)| !equivalent_style_spec(first, candidate))
            {
                conflicting_face_targets.insert(target.to_string());
                let ids = candidates
                    .iter()
                    .map(|(_, id)| (*id).clone())
                    .collect::<BTreeSet<_>>();
                conflicted_binding_ids.extend(ids.iter().cloned());
                target_conflicts.insert(("face".into(), target.into()), ids);
            } else {
                face_colors.insert(target, first);
            }
        }
        self.conflicted_appearance_bindings
            .extend(conflicted_binding_ids);
        self.appearance_binding_target_conflicts
            .extend(target_conflicts);
        for body in &ir.model.bodies {
            if !conflicting_body_targets.contains(body.id.as_str()) {
                if let Some(color) = body.color {
                    body_colors.entry(body.id.as_str()).or_insert(ColorSpec {
                        color,
                        appearance: None,
                        hidden: false,
                    });
                }
            }
        }
        for face in &ir.model.faces {
            if !conflicting_face_targets.contains(face.id.as_str()) {
                if let Some(color) = face.color {
                    face_colors.entry(face.id.as_str()).or_insert(ColorSpec {
                        color,
                        appearance: None,
                        hidden: false,
                    });
                }
            }
        }

        let mut face_body: HashMap<&str, &str> = HashMap::new();
        for region in &ir.model.regions {
            let body = region.body.as_str();
            for shell_id in &region.shells {
                let Some(shell) = self.shells.get(shell_id.as_str()).copied() else {
                    continue;
                };
                for face in shell.faces() {
                    face_body.insert(face.as_str(), body);
                }
            }
        }

        // Face-level STYLED_ITEM only: OCCT/VTK viewers read colors from
        // ADVANCED_FACE and ignore MANIFOLD_SOLID_BREP. Bodies with no face use
        // the shape carrier for whole-body color.
        let mut style_refs: HashMap<String, Ref> = HashMap::new();
        let mut styled = Vec::new();
        let mut faces: Vec<(String, Ref)> = self
            .face_step_refs
            .iter()
            .map(|(id, r)| (id.clone(), *r))
            .collect();
        faces.sort_by(|a, b| a.0.cmp(&b.0));
        let mut styled_bodies: BTreeSet<&str> = BTreeSet::new();
        for (face_id, face) in &faces {
            if conflicting_face_targets.contains(face_id) {
                continue;
            }
            let own = face_colors.get(face_id.as_str()).copied();
            let body = face_body.get(face_id.as_str()).copied();
            let inherited = body.and_then(|b| body_colors.get(b).copied());
            let Some(spec) = own.or(inherited) else {
                continue;
            };
            // The body color is only counted as represented when a face without
            // its own override receives it.
            if own.is_none() {
                if let Some(b) = body {
                    styled_bodies.insert(b);
                }
            }
            if own.is_some() {
                if let Some(binding_ids) = face_candidates.get(face_id.as_str()) {
                    self.written_appearance_bindings
                        .extend(binding_ids.iter().map(|(_, id)| (*id).clone()));
                }
            } else if let Some(body_id) = body {
                if let Some(binding_ids) = body_candidates.get(body_id) {
                    self.written_appearance_bindings
                        .extend(binding_ids.iter().map(|(_, id)| (*id).clone()));
                }
            }
            let name = spec
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
                .unwrap_or("");
            let style = self.surface_style(spec.color, name, &mut style_refs);
            styled.push(self.emit_styled_item(style, *face, spec.hidden));
        }
        let mut direct_unstyled = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            if self.written_appearance_bindings.contains(&binding.id)
                || self.conflicted_appearance_bindings.contains(&binding.id)
            {
                continue;
            }
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                continue;
            };
            let Some(color) = appearance.base_color else {
                continue;
            };
            let (target, style_kind) = match &binding.target {
                AppearanceTarget::Face(id) => (
                    self.face_step_refs.get(id.as_str()).copied(),
                    StyleKind::Surface,
                ),
                AppearanceTarget::Surface(id) => (
                    self.surface_refs
                        .get(id.as_str())
                        .map(|surface| surface.reference),
                    StyleKind::Surface,
                ),
                AppearanceTarget::Curve(id) => {
                    (self.curve_refs.get(id.as_str()).copied(), StyleKind::Curve)
                }
                AppearanceTarget::Edge(id) => {
                    (self.edge_refs.get(id.as_str()).copied(), StyleKind::Curve)
                }
                AppearanceTarget::Point(id) => {
                    (self.point_refs.get(id.as_str()).copied(), StyleKind::Point)
                }
                AppearanceTarget::Vertex(id) => {
                    (self.vertex_refs.get(id.as_str()).copied(), StyleKind::Point)
                }
                AppearanceTarget::Tessellation(id) => (
                    self.tessellation_step_refs.get(id).copied(),
                    StyleKind::Surface,
                ),
                AppearanceTarget::Body(_) | AppearanceTarget::Source { .. } => continue,
            };
            let Some(target) = target else {
                let target_id = match &binding.target {
                    AppearanceTarget::Face(id) => id.as_str().to_owned(),
                    AppearanceTarget::Surface(id) => id.as_str().to_owned(),
                    AppearanceTarget::Curve(id) => id.as_str().to_owned(),
                    AppearanceTarget::Edge(id) => id.as_str().to_owned(),
                    AppearanceTarget::Point(id) => id.as_str().to_owned(),
                    AppearanceTarget::Vertex(id) => id.as_str().to_owned(),
                    AppearanceTarget::Tessellation(id) => id.clone(),
                    AppearanceTarget::Body(_) | AppearanceTarget::Source { .. } => continue,
                };
                direct_unstyled.insert(target_id);
                continue;
            };
            let name = appearance.name.as_deref().unwrap_or("");
            let style = match style_kind {
                StyleKind::Surface => self.surface_style(color, name, &mut style_refs),
                StyleKind::Curve => self.curve_style(color, name, &mut style_refs),
                StyleKind::Point => self.point_style(color, name, &mut style_refs),
            };
            self.written_appearance_bindings.insert(binding.id.clone());
            styled.push(self.emit_styled_item(
                style,
                target,
                hidden_binding_ids.contains(binding.id.as_str()),
            ));
        }
        for (body_id, spec) in &body_colors {
            if styled_bodies.contains(body_id) {
                continue;
            }
            let mut targets = self
                .body_item_refs
                .get(*body_id)
                .cloned()
                .unwrap_or_default();
            if targets.is_empty() {
                targets.extend(self.body_shape_refs.get(*body_id).copied());
            }
            if targets.is_empty() {
                continue;
            }
            if let Some(binding_ids) = body_candidates.get(*body_id) {
                self.written_appearance_bindings
                    .extend(binding_ids.iter().map(|(_, id)| (*id).clone()));
            }
            let name = spec
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
                .unwrap_or("");
            let style = if self
                .bodies
                .get(*body_id)
                .is_some_and(|body| body.kind == BodyKind::Wire)
            {
                if spec.appearance.is_none() && spec.color.a() < 1.0 {
                    self.loss(
                        StepLossCode::WireBodyTransparencyOmitted,
                        format!(
                            "wire body '{body_id}' direct color transparency was omitted from STEP CURVE_STYLE"
                        ),
                    );
                }
                self.curve_style(spec.color, name, &mut style_refs)
            } else {
                self.surface_style(spec.color, name, &mut style_refs)
            };
            styled.extend(
                targets
                    .into_iter()
                    .map(|target| self.emit_styled_item(style, target, spec.hidden)),
            );
            styled_bodies.insert(body_id);
        }
        // A color is unrepresented when no emitted ADVANCED_FACE could carry it:
        // a face override whose face was skipped, or a body whose faces were all
        // skipped (hidden bodies or faces without an explicit STEP surface),
        // and no whole-body carrier was emitted.
        let emitted: BTreeSet<&str> = self.face_step_refs.keys().map(String::as_str).collect();
        let mut unstyled_targets = face_colors
            .keys()
            .filter(|id| !emitted.contains(*id))
            .map(|id| (*id).to_string())
            .collect::<BTreeSet<_>>();
        unstyled_targets.extend(
            body_colors
                .keys()
                .filter(|id| !styled_bodies.contains(*id))
                .map(|id| (*id).to_string()),
        );
        unstyled_targets.extend(direct_unstyled);
        self.unstyled_colors = unstyled_targets.len();
        if styled.is_empty() {
            return;
        }
        self.emitter.emit(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION",
            &format!("'',{},{context}", refs(&styled)),
        );
    }

    fn emit_styled_item(&mut self, style: Ref, target: Ref, hidden: bool) -> Ref {
        let item = self
            .emitter
            .emit("STYLED_ITEM", &format!("'color',({style}),{target}"));
        if hidden {
            self.hidden_appearance_items.push(item);
        }
        item
    }

    fn surface_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            self.emitter.real(f64::from(color.r())),
            self.emitter.real(f64::from(color.g())),
            self.emitter.real(f64::from(color.b()))
        );
        let key = format!("surface:{name}:{rgb}:{}", color.a().to_bits());
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let fill_colour = self
            .emitter
            .emit("FILL_AREA_STYLE_COLOUR", &format!("'',{colour}"));
        let fill = self
            .emitter
            .emit("FILL_AREA_STYLE", &format!("'',({fill_colour})"));
        let style_fill = self
            .emitter
            .emit("SURFACE_STYLE_FILL_AREA", &fill.to_string());
        let mut styles = vec![style_fill];
        if color.a() < 1.0 {
            let transparency = self.emitter.emit(
                "SURFACE_STYLE_TRANSPARENT",
                &self.emitter.real(1.0 - f64::from(color.a())),
            );
            let rendering = self.emitter.emit(
                "SURFACE_STYLE_RENDERING_WITH_PROPERTIES",
                &format!(".CONSTANT_SHADING.,{colour},{}", refs(&[transparency])),
            );
            styles.push(rendering);
        }
        let side = self
            .emitter
            .emit("SURFACE_SIDE_STYLE", &format!("'',{}", refs(&styles)));
        let usage = self
            .emitter
            .emit("SURFACE_STYLE_USAGE", &format!(".BOTH.,{side}"));
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({usage})"));
        cache.insert(key, assignment);
        assignment
    }

    fn curve_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            self.emitter.real(f64::from(color.r())),
            self.emitter.real(f64::from(color.g())),
            self.emitter.real(f64::from(color.b()))
        );
        let key = format!("curve:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let font = self
            .emitter
            .emit("DRAUGHTING_PRE_DEFINED_CURVE_FONT", &string("continuous"));
        let curve = self.emitter.emit(
            "CURVE_STYLE",
            &format!("'',{font},POSITIVE_LENGTH_MEASURE(0.1),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({curve})"));
        cache.insert(key, assignment);
        assignment
    }

    fn point_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            self.emitter.real(f64::from(color.r())),
            self.emitter.real(f64::from(color.g())),
            self.emitter.real(f64::from(color.b()))
        );
        let key = format!("point:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let point = self.emitter.emit(
            "POINT_STYLE",
            &format!("'',.DOT.,POSITIVE_LENGTH_MEASURE(1.),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({point})"));
        cache.insert(key, assignment);
        assignment
    }

    fn emit_layers(&mut self) {
        for layer in self.ir.model.presentation_layers.clone() {
            let mut assigned = Vec::new();
            let mut unsupported = 0usize;
            for item in layer.items {
                let references = match item {
                    PresentationItem::Body { body } => self
                        .body_item_refs
                        .get(body.as_str())
                        .cloned()
                        .filter(|references| !references.is_empty())
                        .or_else(|| {
                            self.body_shape_refs
                                .get(body.as_str())
                                .copied()
                                .map(|reference| vec![reference])
                        })
                        .unwrap_or_default(),
                    PresentationItem::Face { face } => self
                        .face_step_refs
                        .get(face.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Edge { edge } => self
                        .edge_refs
                        .get(edge.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Vertex { vertex } => self
                        .vertex_refs
                        .get(vertex.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Curve { curve } => self
                        .curve_refs
                        .get(curve.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Surface { surface } => self
                        .surface_refs
                        .get(surface.as_str())
                        .map(|surface| surface.reference)
                        .into_iter()
                        .collect(),
                    PresentationItem::Product { product } => self
                        .product_step_refs
                        .get(product.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Occurrence { occurrence } => self
                        .occurrence_step_refs
                        .get(occurrence.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Pmi { annotation } => self
                        .pmi_step_refs
                        .get(annotation.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Point { point } => self
                        .point_refs
                        .get(point.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Source { .. } => Vec::new(),
                    PresentationItem::Tessellation { tessellation } => self
                        .tessellation_step_refs
                        .get(&tessellation)
                        .copied()
                        .into_iter()
                        .collect(),
                };
                if references.is_empty() {
                    unsupported += 1;
                } else {
                    assigned.extend(references);
                }
            }
            if unsupported > 0 {
                self.loss(
                    StepLossCode::LayerItemWithoutCarrier,
                    format!(
                        "layer '{}' has {unsupported} item(s) without a writable STEP carrier",
                        layer.name
                    ),
                );
            }
            if assigned.is_empty() {
                if layer.visible == Some(false) {
                    self.hidden_presentation_layers_without_items
                        .insert(layer.id.as_str().to_owned());
                }
            } else {
                let assignment = self.emitter.emit(
                    "PRESENTATION_LAYER_ASSIGNMENT",
                    &format!(
                        "{},{},{}",
                        string(&layer.name),
                        string(layer.description.as_deref().unwrap_or("")),
                        refs(&assigned)
                    ),
                );
                if layer.visible == Some(false) {
                    self.hidden_presentation_layer_items.push(assignment);
                }
            }
        }
    }

    fn emit_product_structure(&mut self) -> Ref {
        let name = self
            .ir
            .model
            .bodies
            .first()
            .and_then(|b| b.name.clone())
            .unwrap_or_else(|| "cadmpeg_model".to_string());

        let (application, protocol, year) = self.schema.application_protocol();
        let app_ctx = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_ctx}",
                string("international standard"),
                string(protocol)
            ),
        );
        let prod_ctx = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_ctx},{}", string("mechanical")),
        );
        let product = self.emitter.emit(
            "PRODUCT",
            &format!("{},{},'',({prod_ctx})", string(&name), string(&name)),
        );
        let formation = self
            .emitter
            .emit("PRODUCT_DEFINITION_FORMATION", &format!("'','',{product}"));
        let pd_ctx = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_ctx},{}",
                string("part definition"),
                string("design")
            ),
        );
        let product_def = self.emitter.emit(
            "PRODUCT_DEFINITION",
            &format!("{},'',{formation},{pd_ctx}", string("design")),
        );
        self.emitter
            .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{product_def}"))
    }

    fn emit_product_graph(&mut self, context: Ref) {
        let (application, protocol, year) = self.schema.application_protocol();
        let app_context = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_context}",
                string("international standard"),
                string(protocol)
            ),
        );
        let product_context = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_context},{}", string("mechanical")),
        );
        let definition_context = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_context},{}",
                string("part definition"),
                string("design")
            ),
        );

        let ir = self.ir;
        let products = &ir.model.product_definitions;
        let occurrences = &ir.model.occurrences;
        let Ok(graph) = AssemblyGraph::new(occurrences) else {
            self.loss(
                StepLossCode::AssemblyGraphInvalid,
                "assembly occurrence graph is invalid".into(),
            );
            return;
        };
        let occurrence_products = occurrences
            .iter()
            .filter_map(|occurrence| match &occurrence.prototype {
                PrototypeReference::Local { definition } => {
                    Some((occurrence.id.clone(), definition.clone()))
                }
                PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => None,
            })
            .collect::<HashMap<OccurrenceId, ProductDefinitionId>>();
        let mut product_origins = HashMap::<ProductDefinitionId, Ref>::new();
        for product in products {
            product_origins.insert(
                product.id.clone(),
                geometry::placement(
                    &mut self.emitter,
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::units::UnitVector3::Z_AXIS,
                    cadmpeg_ir::units::UnitVector3::X_AXIS,
                ),
            );
        }
        let mut representation_placements = HashMap::<ProductDefinitionId, Vec<Ref>>::new();
        let mut occurrence_placements = HashMap::<OccurrenceId, (Ref, Ref)>::new();
        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                continue;
            };
            let Some(parent_product) = occurrence_products.get(parent) else {
                continue;
            };
            let Some(child_product) = occurrence_products.get(&occurrence.id) else {
                continue;
            };
            let Some(&from) = product_origins.get(child_product) else {
                continue;
            };
            let transform = match occurrence.effective_transform() {
                Ok(transform) => transform,
                Err(error) => {
                    self.loss(
                        StepLossCode::AssemblyGraphInvalid,
                        format!("occurrence '{}': {error}", occurrence.id),
                    );
                    return;
                }
            };
            let Some(frame) = transform.proper_rigid_frame() else {
                continue;
            };
            if occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get) != [1.0; 3] {
                continue;
            }
            let rows = transform.rows();
            let to = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                *frame.axis(),
                *frame.reference(),
            );
            representation_placements
                .entry(parent_product.clone())
                .or_default()
                .push(to);
            occurrence_placements.insert(occurrence.id.clone(), (from, to));
        }
        let mut definitions = HashMap::<ProductDefinitionId, Ref>::new();
        let mut representations = HashMap::<ProductDefinitionId, Ref>::new();
        for product in products {
            let product_id = product
                .part_number
                .as_deref()
                .or(product.source_name.as_deref())
                .unwrap_or(product.id.as_str());
            let name = product
                .label
                .as_deref()
                .or(product.source_name.as_deref())
                .unwrap_or(product_id);
            let description = product.description.as_deref().unwrap_or("");
            let product_ref = self.emitter.emit(
                "PRODUCT",
                &format!(
                    "{},{},{},({product_context})",
                    string(product_id),
                    string(name),
                    string(description)
                ),
            );
            self.product_step_refs
                .insert(product.id.as_str().to_owned(), product_ref);
            let formation = self.emitter.emit(
                "PRODUCT_DEFINITION_FORMATION",
                &format!("'','',{product_ref}"),
            );
            let definition = self.emitter.emit(
                "PRODUCT_DEFINITION",
                &format!(
                    "{},{},{formation},{definition_context}",
                    string(product_id),
                    string(description)
                ),
            );
            let shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{definition}"));
            self.default_product_definition_shape.get_or_insert(shape);
            let mut body_items = product
                .bodies
                .iter()
                .flat_map(|body| {
                    self.body_item_refs
                        .get(body.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect::<Vec<_>>();
            if let Some(origin) = product_origins.get(&product.id) {
                body_items.push(*origin);
            }
            if let Some(placements) = representation_placements.get(&product.id) {
                body_items.extend(placements);
            }
            let representation = self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&body_items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{shape},{representation}"),
            );
            definitions.insert(product.id.clone(), definition);
            representations.insert(product.id.clone(), representation);
        }

        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                let transform = match occurrence.effective_transform() {
                    Ok(transform) => transform,
                    Err(error) => {
                        self.loss(
                            StepLossCode::AssemblyGraphInvalid,
                            format!("occurrence '{}': {error}", occurrence.id),
                        );
                        return;
                    }
                };
                if !is_identity(&transform.rows())
                    || occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get) != [1.0; 3]
                {
                    self.loss(
                        StepLossCode::RootOccurrencePlacementNotRepresentable,
                        format!(
                            "root occurrence '{}' has a placement or scale that is not representable",
                            occurrence.id
                        ),
                    );
                }
                continue;
            };
            let Some(parent_occurrence) = graph.occurrence(parent) else {
                self.loss(
                    StepLossCode::OccurrenceUnresolvedParent,
                    format!("occurrence '{}' has an unresolved parent", occurrence.id),
                );
                continue;
            };
            let Some(parent_product) = occurrence_products.get(&parent_occurrence.id) else {
                self.missing_parent_products
                    .insert(occurrence.id.as_str().to_owned());
                continue;
            };
            let Some(child_product) = occurrence_products.get(&occurrence.id) else {
                self.loss(
                    StepLossCode::OccurrenceNoLocalProduct,
                    format!(
                        "occurrence '{}' has no local product definition",
                        occurrence.id
                    ),
                );
                continue;
            };
            let Some((
                &parent_definition,
                &child_definition,
                &parent_representation,
                &child_representation,
            )) = definitions
                .get(parent_product)
                .zip(definitions.get(child_product))
                .zip(representations.get(parent_product))
                .zip(representations.get(child_product))
                .map(|(((a, b), c), d)| (a, b, c, d))
            else {
                continue;
            };
            let transform = match occurrence.effective_transform() {
                Ok(transform) => transform,
                Err(error) => {
                    self.loss(
                        StepLossCode::AssemblyGraphInvalid,
                        format!("occurrence '{}': {error}", occurrence.id),
                    );
                    return;
                }
            };
            if !transform.is_proper_rigid()
                || occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get) != [1.0; 3]
            {
                self.loss(
                    StepLossCode::OccurrencePlacementNotRigid,
                    format!("occurrence '{}' placement is not rigid", occurrence.id),
                );
                continue;
            }
            let occurrence_name = occurrence.name.as_deref().unwrap_or(occurrence.id.as_str());
            let usage = self.emitter.emit(
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
                &format!(
                    "{},{},'',{parent_definition},{child_definition},$",
                    string(occurrence.id.as_str()),
                    string(occurrence_name)
                ),
            );
            self.occurrence_step_refs
                .insert(occurrence.id.as_str().to_owned(), usage);
            let usage_shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{usage}"));
            let Some(&(from, to)) = occurrence_placements.get(&occurrence.id) else {
                continue;
            };
            let transform = self
                .emitter
                .emit("ITEM_DEFINED_TRANSFORMATION", &format!("'','',{from},{to}"));
            let relationship = self.emitter.emit_raw(
                "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
                &format!(
                    "( REPRESENTATION_RELATIONSHIP('','',{child_representation},{parent_representation}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION({transform}) SHAPE_REPRESENTATION_RELATIONSHIP() )"
                ),
            );
            self.emitter.emit(
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
                &format!("{relationship},{usage_shape}"),
            );
        }
    }

    fn emit_context(&mut self) -> Ref {
        let len = self.emit_length_unit();
        let angle = self.emit_angle_unit();
        let solid = self.emitter.emit_raw(
            "SOLID_ANGLE_UNIT",
            "( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )",
        );
        let unc = self.emitter.emit(
            "UNCERTAINTY_MEASURE_WITH_UNIT",
            &format!(
                "LENGTH_MEASURE({}),{len},{},{}",
                self.emitter.real(self.ir.tolerances.linear.get()),
                string("distance_accuracy_value"),
                string("maximum model space distance")
            ),
        );
        self.emitter.emit_raw(
            "GEOMETRIC_REPRESENTATION_CONTEXT",
            &format!(
                "( GEOMETRIC_REPRESENTATION_CONTEXT(3) \
                 GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc})) \
                 GLOBAL_UNIT_ASSIGNED_CONTEXT(({len},{angle},{solid})) \
                 REPRESENTATION_CONTEXT('Context','3D') )"
            ),
        )
    }

    fn emit_length_unit(&mut self) -> Ref {
        if let Some(unit) = self.length_unit {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            "LENGTH_UNIT",
            "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )",
        );
        self.length_unit = Some(unit);
        unit
    }

    fn emit_angle_unit(&mut self) -> Ref {
        if let Some(unit) = self.angle_unit {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            "PLANE_ANGLE_UNIT",
            "( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )",
        );
        self.angle_unit = Some(unit);
        unit
    }

    fn emit_ratio_unit(&mut self) -> Ref {
        if let Some(unit) = self.ratio_unit {
            return unit;
        }
        let unit = self
            .emitter
            .emit_raw("RATIO_UNIT", "( NAMED_UNIT(*) RATIO_UNIT() )");
        self.ratio_unit = Some(unit);
        unit
    }

    /// Emit one shape item per region; visibility is represented separately
    /// when the target application protocol supports `INVISIBILITY`.
    fn emit_shape_items(&mut self, context: Ref) -> Vec<Ref> {
        let mut items = Vec::new();
        let ir = self.ir;
        for region in &ir.model.regions {
            let body_kind = self
                .bodies
                .get(region.body.as_str())
                .map_or(BodyKind::General, |body| body.kind);
            let has_surface_topology = region.shells.iter().any(|shell_id| {
                self.shells
                    .get(shell_id.as_str())
                    .is_some_and(|shell| !shell.faces().is_empty())
            });
            let has_wire_topology = region.shells.iter().any(|shell_id| {
                self.shells.get(shell_id.as_str()).is_some_and(|shell| {
                    !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty()
                })
            });
            let mixed_wire = body_kind == BodyKind::General && has_wire_topology;
            if body_kind == BodyKind::Wire
                || (body_kind == BodyKind::General && !has_surface_topology)
            {
                if let Some(item) = self.emit_wire_region(region) {
                    let shape_item = self.place_body_item(&region.body, item, context);
                    items.push(shape_item);
                    self.body_shape_refs
                        .entry(region.body.as_str().to_owned())
                        .or_insert(shape_item);
                    self.body_item_refs
                        .entry(region.body.as_str().to_owned())
                        .or_default()
                        .push(shape_item);
                    self.body_step_item_refs
                        .entry(region.body.as_str().to_owned())
                        .or_default()
                        .push(item);
                    self.body_step_refs
                        .entry(region.body.as_str().to_owned())
                        .or_insert(item);
                } else {
                    self.empty_wire_regions
                        .insert(region.id.as_str().to_owned());
                }
                continue;
            }
            let closed = body_kind == BodyKind::Solid;
            let Some((outer_id, void_ids)) = region.shells.split_first() else {
                self.empty_regions.insert(region.id.as_str().to_owned());
                continue;
            };
            let Some(outer) = self.emit_shell(outer_id.as_str(), closed) else {
                self.topology_relation_loss(
                    format!("region:{}:outer-shell:{}", region.id, outer_id),
                    StepLossCode::RegionNoWritableOuterShell,
                    format!("region {} has no writable outer shell", region.id),
                );
                continue;
            };
            let mut voids = Vec::new();
            for sid in void_ids {
                if let Some(void) = self.emit_shell(sid.as_str(), closed) {
                    voids.push(void);
                } else {
                    self.topology_relation_loss(
                        format!("region:{}:void-shell:{}", region.id, sid),
                        StepLossCode::RegionOmittedVoidShell,
                        format!(
                            "region {} omitted void shell {} because it has no writable faces",
                            region.id, sid
                        ),
                    );
                }
            }
            let mut shell_refs = Vec::new();
            shell_refs.push(outer);
            shell_refs.extend_from_slice(&voids);
            let item = if !closed {
                self.emitter.emit(
                    "SHELL_BASED_SURFACE_MODEL",
                    &format!("'',{}", refs(&shell_refs)),
                )
            } else if voids.is_empty() {
                self.emitter
                    .emit("MANIFOLD_SOLID_BREP", &format!("'',{outer}"))
            } else {
                let void_refs: Vec<Ref> = voids
                    .iter()
                    .map(|s| {
                        self.emitter
                            .emit("ORIENTED_CLOSED_SHELL", &format!("'',*,{s},.F."))
                    })
                    .collect();
                self.emitter.emit(
                    "BREP_WITH_VOIDS",
                    &format!("'',{outer},{}", refs(&void_refs)),
                )
            };
            let shape_item = self.place_body_item(&region.body, item, context);
            items.push(shape_item);
            self.body_shape_refs
                .entry(region.body.as_str().to_owned())
                .or_insert(shape_item);
            self.body_item_refs
                .entry(region.body.as_str().to_owned())
                .or_default()
                .push(shape_item);
            self.body_step_item_refs
                .entry(region.body.as_str().to_owned())
                .or_default()
                .push(item);
            self.body_step_refs
                .entry(region.body.as_str().to_owned())
                .or_insert(if closed { item } else { outer });
            if mixed_wire {
                if let Some(item) = self.emit_wire_region(region) {
                    let shape_item = self.place_body_item(&region.body, item, context);
                    items.push(shape_item);
                    self.body_shape_refs
                        .entry(region.body.as_str().to_owned())
                        .or_insert(shape_item);
                    self.body_item_refs
                        .entry(region.body.as_str().to_owned())
                        .or_default()
                        .push(shape_item);
                    self.body_step_item_refs
                        .entry(region.body.as_str().to_owned())
                        .or_default()
                        .push(item);
                    self.body_step_refs
                        .entry(region.body.as_str().to_owned())
                        .or_insert(item);
                } else {
                    self.empty_wire_regions
                        .insert(region.id.as_str().to_owned());
                }
            }
        }
        items
    }

    fn place_body_item(
        &mut self,
        body_id: &cadmpeg_ir::ids::BodyId,
        item: Ref,
        context: Ref,
    ) -> Ref {
        let transform = self
            .bodies
            .get(body_id.as_str())
            .and_then(|body| body.transform);
        let Some(transform) = transform.filter(|transform| !is_identity(&transform.rows())) else {
            return item;
        };
        let Some(frame) = transform.proper_rigid_frame() else {
            self.loss(
                StepLossCode::BodyNonRigidTransform,
                format!("body '{body_id}' carries a non-rigid transform"),
            );
            return item;
        };
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        );
        let representation = self.emitter.emit(
            "SHAPE_REPRESENTATION",
            &format!("'body-local',({item}),{context}"),
        );
        let map = self
            .emitter
            .emit("REPRESENTATION_MAP", &format!("{origin},{representation}"));
        let rows = transform.rows();
        let target = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
            *frame.axis(),
            *frame.reference(),
        );
        self.emitter.emit(
            "MAPPED_ITEM",
            &format!("'cadmpeg body placement',{map},{target}"),
        )
    }

    fn emit_visibility(&mut self) {
        if !self.schema.supports_visibility() {
            let hidden = self
                .ir
                .model
                .bodies
                .iter()
                .filter(|body| body.visible == Some(false))
                .count();
            if hidden != 0 {
                self.loss(
                    StepLossCode::HiddenBodyVisibilityUnsupported,
                    format!(
                        "{hidden} hidden body visibility assignment(s) are unsupported by \
                         {}; this write target does not emit INVISIBILITY",
                        self.schema.file_schema()
                    ),
                );
            }
            return;
        }
        let mut hidden = Vec::new();
        let mut hidden_without_items = Vec::new();
        for body in &self.ir.model.bodies {
            if body.visible != Some(false) {
                continue;
            }
            if let Some(references) = self.body_step_item_refs.get(body.id.as_str()) {
                if !references.is_empty() {
                    hidden.extend(references.iter().copied());
                    continue;
                }
            }
            if let Some(reference) = self.body_step_refs.get(body.id.as_str()).copied() {
                hidden.push(reference);
            } else {
                hidden_without_items.push(body.id.as_str().to_owned());
            }
        }
        self.hidden_bodies_without_items
            .extend(hidden_without_items);
        if !hidden.is_empty() {
            self.emitter.emit("INVISIBILITY", &refs(&hidden));
        }
    }

    fn emit_appearance_visibility(&mut self) {
        let hidden_bindings = self
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.visible == Some(false))
            .count();
        if hidden_bindings == 0 {
            return;
        }
        if !self.schema.supports_visibility() {
            self.loss(
                StepLossCode::HiddenAppearanceVisibilityUnsupported,
                format!(
                    "{hidden_bindings} hidden appearance binding visibility assignment(s) \
                     are unsupported by {}; this write target does not emit INVISIBILITY",
                    self.schema.file_schema()
                ),
            );
            return;
        }
        if !self.hidden_appearance_items.is_empty() {
            self.emitter
                .emit("INVISIBILITY", &refs(&self.hidden_appearance_items));
        }
    }

    fn emit_layer_visibility(&mut self) {
        let hidden_layers = self
            .ir
            .model
            .presentation_layers
            .iter()
            .filter(|layer| layer.visible == Some(false))
            .count();
        if hidden_layers == 0 {
            return;
        }
        if !self.schema.supports_visibility() {
            self.loss(
                StepLossCode::HiddenPresentationLayerVisibilityUnsupported,
                format!(
                    "{hidden_layers} hidden presentation layer visibility assignment(s) are \
                     unsupported by {}; this write target does not emit INVISIBILITY",
                    self.schema.file_schema()
                ),
            );
            return;
        }
        if !self.hidden_presentation_layer_items.is_empty() {
            self.emitter
                .emit("INVISIBILITY", &refs(&self.hidden_presentation_layer_items));
        }
    }

    fn emit_wire_region(&mut self, region: &cadmpeg_ir::topology::Region) -> Option<Ref> {
        let mut shells = Vec::new();
        for shell_id in &region.shells {
            if let Some(shell) = self.shells.get(shell_id.as_str()).copied().cloned() {
                shells.push(shell);
            } else {
                self.missing_wire_shells
                    .insert((region.id.as_str().to_owned(), shell_id.as_str().to_owned()));
            }
        }
        let mut connected_sets = Vec::new();
        for shell in shells {
            if !shell.free_vertices().is_empty() {
                self.loss(
                    StepLossCode::WireShellFreeVertices,
                    format!(
                        "wire shell '{}' has {} free vertex/vertices without an edge-based STEP carrier",
                        shell.id,
                        shell.free_vertices().len()
                    ),
                );
            }
            let edges = shell
                .wire_edges()
                .iter()
                .filter_map(|id| {
                    let emitted = self.emit_edge(id.as_str())?;
                    self.edges.get(id.as_str()).map(|edge| (emitted, *edge))
                })
                .collect::<Vec<_>>();
            for component in wire::connected_components(&edges) {
                connected_sets.push(
                    self.emitter
                        .emit("CONNECTED_EDGE_SET", &format!("'',{}", refs(&component))),
                );
            }
        }
        if connected_sets.is_empty() {
            return None;
        }
        Some(self.emitter.emit(
            "EDGE_BASED_WIREFRAME_MODEL",
            &format!("'',{}", refs(&connected_sets)),
        ))
    }

    fn emit_standalone_geometry(&mut self) -> Vec<Ref> {
        // Ownership is a property of the source/model graph, not of successful
        // emission. A failed face must never expose its full support as a root.
        let owned_surfaces: BTreeSet<_> = self
            .ir
            .model
            .faces
            .iter()
            .map(|face| face.surface.as_str())
            .collect();
        let owned_curves: BTreeSet<_> = self
            .ir
            .model
            .edges
            .iter()
            .filter_map(|edge| edge.carrier.curve())
            .map(cadmpeg_ir::ids::CurveId::as_str)
            .chain(
                self.ir
                    .model
                    .coedges
                    .iter()
                    .filter_map(|coedge| coedge.use_curve.as_ref())
                    .map(|use_| use_.curve.as_str()),
            )
            .collect();
        let owned_points: BTreeSet<_> = self
            .ir
            .model
            .vertices
            .iter()
            .map(|vertex| vertex.point.as_str())
            .collect();
        for surface in &self.ir.model.surfaces {
            if !self.surface_refs.contains_key(surface.id.as_str())
                && (owned_surfaces.contains(surface.id.as_str())
                    || ownership::is_support(
                        surface.id.as_str(),
                        surface.source_object.as_ref(),
                        &self.construction_supports,
                    ))
            {
                self.unwritten_geometry_carriers
                    .insert(surface.id.as_str().to_owned());
            }
        }
        for curve in &self.ir.model.curves {
            if !self.curve_refs.contains_key(curve.id.as_str())
                && (owned_curves.contains(curve.id.as_str())
                    || ownership::is_support(
                        curve.id.as_str(),
                        curve.source_object.as_ref(),
                        &self.construction_supports,
                    ))
            {
                self.unwritten_geometry_carriers
                    .insert(curve.id.as_str().to_owned());
            }
        }
        for point in &self.ir.model.points {
            if !self.point_refs.contains_key(point.id.as_str())
                && (owned_points.contains(point.id.as_str())
                    || ownership::is_support(
                        point.id.as_str(),
                        point.source_object.as_ref(),
                        &self.construction_supports,
                    ))
            {
                self.unwritten_geometry_carriers
                    .insert(point.id.as_str().to_owned());
            }
        }
        let surface_ids = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                !self.surface_refs.contains_key(surface.id.as_str())
                    && !owned_surfaces.contains(surface.id.as_str())
                    && !ownership::is_support(
                        surface.id.as_str(),
                        surface.source_object.as_ref(),
                        &self.construction_supports,
                    )
            })
            .map(|surface| surface.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let mut members = Vec::new();
        let mut has_surfaces = false;
        for surface_id in surface_ids {
            if let Some(reference) = self.emit_surface(&surface_id) {
                let ranges =
                    self.procedural_surfaces
                        .get(surface_id.as_str())
                        .and_then(|procedural| match procedural.definition() {
                            ProceduralSurfaceDefinition::Exact(payload) => match payload.spline() {
                                cadmpeg_ir::geometry::ExactSpline::Legacy { ranges, .. } => {
                                    Some(ranges.map(|range| {
                                        range.map(cadmpeg_ir::scalar::FiniteReal::get)
                                    }))
                                }
                                cadmpeg_ir::geometry::ExactSpline::Revision { .. } => None,
                            },
                            _ => None,
                        });
                let reference = match ranges {
                    Some(ranges) => self.emit_rectangular_trim(
                        reference,
                        ranges,
                        [true, true],
                        self.surface_chart_reversed(&surface_id),
                    ),
                    None => reference,
                };
                members.push(reference);
                has_surfaces = true;
            } else {
                self.unwritten_geometry_carriers.insert(surface_id);
            }
        }
        let curve_ids = self
            .ir
            .model
            .curves
            .iter()
            .filter(|curve| {
                !self.curve_refs.contains_key(curve.id.as_str())
                    && !owned_curves.contains(curve.id.as_str())
                    && !ownership::is_support(
                        curve.id.as_str(),
                        curve.source_object.as_ref(),
                        &self.construction_supports,
                    )
            })
            .map(|curve| curve.id.as_str().to_owned())
            .collect::<Vec<_>>();
        for curve_id in curve_ids {
            if let Some(reference) = self.emit_curve(&curve_id) {
                let reference = if let Some(range) = self
                    .curves
                    .get(curve_id.as_str())
                    .and_then(|curve| curve.parameter_range)
                {
                    let [start, end] = range.endpoints();
                    self.emitter.emit("TRIMMED_CURVE", &format!("'',{reference},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),.T.,.PARAMETER.", self.emitter.real(start), self.emitter.real(end)))
                } else {
                    reference
                };
                members.push(reference);
            } else {
                self.unwritten_geometry_carriers.insert(curve_id);
            }
        }
        let point_ids = self
            .ir
            .model
            .points
            .iter()
            .filter(|point| {
                !self.point_refs.contains_key(point.id.as_str())
                    && !owned_points.contains(point.id.as_str())
                    && !ownership::is_support(
                        point.id.as_str(),
                        point.source_object.as_ref(),
                        &self.construction_supports,
                    )
            })
            .map(|point| point.id.as_str().to_owned())
            .collect::<Vec<_>>();
        for point_id in point_ids {
            let Some(point) = self.points.get(point_id.as_str()).copied() else {
                continue;
            };
            let reference = geometry::point(&mut self.emitter, point.position().get());
            self.point_refs.insert(point_id, reference);
            members.push(reference);
        }
        if members.is_empty() {
            Vec::new()
        } else {
            vec![self.emitter.emit(
                if has_surfaces {
                    "GEOMETRIC_SET"
                } else {
                    "GEOMETRIC_CURVE_SET"
                },
                &format!("'',{}", refs(&members)),
            )]
        }
    }

    fn emit_tessellations(&mut self, context: Ref) {
        if self.ir.model.tessellations.is_empty() {
            return;
        }
        if !self.schema.supports_tessellation() {
            self.loss(
                StepLossCode::TessellationRequiresAp242,
                format!(
                    "{} tessellation(s) require an AP242 target; {} does not carry \
                     tessellated geometry",
                    self.ir.model.tessellations.len(),
                    self.schema.file_schema()
                ),
            );
            return;
        }

        let ir = self.ir;
        let mut representation_items = Vec::new();
        for mesh in &ir.model.tessellations {
            if !mesh.feature_edges().is_empty() {
                self.loss(
                    StepLossCode::TessellationFeatureEdges,
                    format!(
                        "tessellation '{}' feature-edge classification is not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.per_corner_normals().is_empty() {
                self.loss(
                    StepLossCode::TessellationCornerNormals,
                    format!(
                        "tessellation '{}' corner normals are not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.triangle_groups().is_empty() {
                self.loss(
                    StepLossCode::TessellationTriangleGroups,
                    format!(
                        "tessellation '{}' triangle groups are not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.texture_assignments().is_empty() {
                self.loss(
                    StepLossCode::TessellationTextureAssignments,
                    format!(
                        "tessellation '{}' texture assignments are not represented",
                        mesh.id
                    ),
                );
            }
            let mesh_vertices = mesh.vertices();
            let mesh_triangles = mesh.triangles();
            let mesh_normals = mesh.vertex_normals();
            if mesh_vertices.is_empty()
                || mesh_triangles.is_empty()
                || mesh_triangles.iter().flatten().any(|index| {
                    cadmpeg_core::decode::index_from_u32(*index) >= mesh_vertices.len()
                })
                || (!mesh_normals.is_empty() && mesh_normals.len() != mesh_vertices.len())
            {
                self.loss(
                    StepLossCode::TessellationInvalidCardinality,
                    format!(
                        "tessellation '{}' has invalid vertex/index/normal cardinality",
                        mesh.id
                    ),
                );
                continue;
            }
            let coordinates = mesh_vertices
                .iter()
                .map(|point| {
                    format!(
                        "({},{},{})",
                        self.emitter.real(point.x),
                        self.emitter.real(point.y),
                        self.emitter.real(point.z)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let coordinates = self.emitter.emit(
                "COORDINATES_LIST",
                &format!(
                    "{}, {},({coordinates})",
                    string(mesh.id.as_str()),
                    mesh_vertices.len()
                ),
            );
            let normals = if mesh_normals.is_empty() {
                "$".to_string()
            } else {
                format!(
                    "({})",
                    mesh_normals
                        .iter()
                        .map(|normal| format!(
                            "({},{},{})",
                            self.emitter.real(normal.x),
                            self.emitter.real(normal.y),
                            self.emitter.real(normal.z)
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let point_indices = (1..=mesh_vertices.len())
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let linked_body = mesh.body.as_ref().and_then(|body| {
                let link = self.body_step_refs.get(body.as_str()).copied()?;
                let kind = self.bodies.get(body.as_str())?.kind;
                matches!(kind, BodyKind::Solid | BodyKind::Sheet).then_some((kind, link))
            });
            if let Some(body) = &mesh.body {
                if linked_body.is_none() {
                    self.loss(
                        StepLossCode::TessellationBodyLinkUnwritable,
                        format!(
                            "tessellation '{}' body '{}' has no writable AP242 tessellation link",
                            mesh.id, body
                        ),
                    );
                }
            }
            let mut reduced_fields = Vec::new();
            if !mesh.faces.is_empty() {
                reduced_fields.push(format!("{} face ownership link(s)", mesh.faces.len()));
            }
            if mesh.chordal_deflection().is_some() {
                reduced_fields.push("chordal deflection".to_string());
            }
            if !mesh.channels().is_empty() {
                reduced_fields.push(format!("{} data channel(s)", mesh.channels().len()));
            }
            if !reduced_fields.is_empty() {
                self.loss(
                    StepLossCode::TessellationMetadataReduced,
                    format!(
                        "tessellation '{}' reduced unsupported metadata: {}",
                        mesh.id,
                        reduced_fields.join(", ")
                    ),
                );
            }
            let item = if let Some((kind, link)) = linked_body {
                let triangles = mesh_triangles
                    .iter()
                    .map(|triangle| {
                        format!(
                            "({},{},{})",
                            step_triangle_index(triangle[0]),
                            step_triangle_index(triangle[1]),
                            step_triangle_index(triangle[2])
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let face = self.emitter.emit(
                    "TRIANGULATED_FACE",
                    &format!(
                        "{},{coordinates},{},{normals},$,({point_indices}),({triangles})",
                        string(mesh.id.as_str()),
                        mesh_vertices.len()
                    ),
                );
                self.emitter.emit(
                    if kind == BodyKind::Solid {
                        "TESSELLATED_SOLID"
                    } else {
                        "TESSELLATED_SHELL"
                    },
                    &format!("{},({face}),{link}", string(mesh.id.as_str())),
                )
            } else {
                let triangles = mesh_triangles
                    .iter()
                    .map(|triangle| {
                        format!(
                            "({},{},{})",
                            step_triangle_index(triangle[0]),
                            step_triangle_index(triangle[1]),
                            step_triangle_index(triangle[2])
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                self.emitter.emit(
                    "TRIANGULATED_SURFACE_SET",
                    &format!(
                        "{},{coordinates},{},{normals},({point_indices}),({triangles})",
                        string(mesh.id.as_str()),
                        mesh_vertices.len()
                    ),
                )
            };
            self.tessellation_step_refs
                .insert(mesh.id.to_string(), item);
            representation_items.push(item);
        }
        if !representation_items.is_empty() {
            self.emitter.emit(
                "TESSELLATED_SHAPE_REPRESENTATION",
                &format!("'',{},{context}", refs(&representation_items)),
            );
        }
    }

    fn emit_shell(&mut self, shell_id: &str, closed: bool) -> Option<Ref> {
        let shell = self.shells.get(shell_id).copied()?;
        let face_ids: Vec<String> = shell
            .faces()
            .iter()
            .map(|f| f.as_str().to_owned())
            .collect();
        let mut face_refs = Vec::new();
        for fid in &face_ids {
            if let Some(r) = self.emit_face(fid) {
                face_refs.push(r);
            } else {
                let code = match self.faces.get(fid.as_str()).map(|face| &face.loops) {
                    Some(cadmpeg_ir::topology::FaceLoops::Classified { .. }) => {
                        StepLossCode::ShellOmittedOuterFace
                    }
                    _ => StepLossCode::ShellOmittedUnclassifiedFace,
                };
                self.topology_relation_loss(
                    format!("shell:{shell_id}:face:{fid}"),
                    code,
                    format!(
                        "shell {shell_id} omitted face {fid} because the face has no writable topology"
                    ),
                );
            }
        }
        if face_refs.is_empty() {
            return None;
        }
        Some(self.emitter.emit(
            if closed { "CLOSED_SHELL" } else { "OPEN_SHELL" },
            &format!("'',{}", refs(&face_refs)),
        ))
    }

    fn emit_face(&mut self, face_id: &str) -> Option<Ref> {
        let face = self.faces.get(face_id).copied()?;
        let surface_id = face.surface.as_str();
        let same_sense = matches!(face.sense, Sense::Forward);

        // Surface emission admits either a writable construction or a solved
        // carrier. An absent or opaque solved cache cannot veto a construction.
        let Some(surf_ref) = self.emit_surface(surface_id) else {
            self.unknown_surface_faces.insert(face_id.to_string());
            return None;
        };

        let mut bound_refs = Vec::new();
        for loop_id in &face.loops {
            let lid = loop_id.as_str();
            if let Some(loop_ref) = self.emit_loop(lid) {
                let kind = if face.loop_role(loop_id) == LoopBoundaryRole::Outer {
                    "FACE_OUTER_BOUND"
                } else {
                    "FACE_BOUND"
                };
                let b = self.emitter.emit(kind, &format!("'',{loop_ref},.T."));
                bound_refs.push(b);
            } else {
                let code = match face.loop_role(loop_id) {
                    LoopBoundaryRole::Outer => StepLossCode::FaceOmittedOuterLoop,
                    LoopBoundaryRole::Inner => StepLossCode::FaceOmittedInnerLoop,
                    LoopBoundaryRole::Unspecified => StepLossCode::FaceOmittedUnclassifiedLoop,
                };
                self.topology_relation_loss(
                    format!("face:{face_id}:loop:{lid}"),
                    code,
                    format!(
                        "face {face_id} omitted loop {lid} because the loop has no writable topology"
                    ),
                );
            }
        }
        if bound_refs.is_empty() {
            self.topology_relation_loss(
                format!("face:{face_id}:bound-list"),
                StepLossCode::FaceNoWritableBounds,
                format!("face {face_id} has no writable bounds"),
            );
            return None;
        }
        let flag = if same_sense { ".T." } else { ".F." };
        let advanced_face = self.emitter.emit(
            "ADVANCED_FACE",
            &format!(
                "{},{},{surf_ref},{flag}",
                string(face.name.as_deref().unwrap_or("")),
                refs(&bound_refs)
            ),
        );
        self.face_step_refs
            .insert(face_id.to_string(), advanced_face);
        Some(advanced_face)
    }

    fn emit_loop(&mut self, loop_id: &str) -> Option<Ref> {
        let Some(lp) = self.loops.get(loop_id).copied() else {
            self.topology_relation_loss(
                format!("loop:{loop_id}:record"),
                StepLossCode::LoopRecordMissing,
                format!("loop {loop_id} was omitted because its record is missing"),
            );
            return None;
        };
        if let cadmpeg_ir::topology::LoopBoundary::Vertex { vertex, .. } = &lp.boundary {
            let vertex_id = vertex.as_str();
            let Some(vertex) = self.emit_vertex(vertex_id) else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:vertex:{vertex_id}"),
                    StepLossCode::LoopVertexMissing,
                    format!("loop {loop_id} was omitted because vertex {vertex_id} is missing"),
                );
                return None;
            };
            return Some(self.emitter.emit("VERTEX_LOOP", &format!("'',{vertex}")));
        }
        let coedge_ids = self.ordered_loop_coedges(loop_id, lp)?;
        let mut oe_refs = Vec::new();
        for cid in &coedge_ids {
            let Some(coedge) = self.coedges.get(cid.as_str()).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:coedge:{cid}"),
                    StepLossCode::LoopCoedgeRecordMissing,
                    format!("loop {loop_id} omitted coedge {cid} because its record is missing"),
                );
                return None;
            };
            let orientation = matches!(coedge.sense, Sense::Forward);
            let Some(edge_ref) = self.emit_edge(coedge.edge.as_str()) else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:edge:{}", coedge.edge),
                    StepLossCode::LoopEdgeNotWritable,
                    format!(
                        "loop {loop_id} omitted edge {} because the edge is not writable",
                        coedge.edge
                    ),
                );
                return None;
            };
            let flag = if orientation { ".T." } else { ".F." };
            let oe = self
                .emitter
                .emit("ORIENTED_EDGE", &format!("'',*,*,{edge_ref},{flag}"));
            oe_refs.push(oe);
        }
        if oe_refs.is_empty() {
            self.topology_relation_loss(
                format!("loop:{loop_id}:edge-list"),
                StepLossCode::LoopNoWritableEdges,
                format!("loop {loop_id} has no writable edges"),
            );
            return None;
        }
        Some(
            self.emitter
                .emit("EDGE_LOOP", &format!("'',{}", refs(&oe_refs))),
        )
    }

    fn ordered_loop_coedges(&mut self, loop_id: &str, lp: &Loop) -> Option<Vec<String>> {
        let mut segments = Vec::new();
        let mut seen = BTreeSet::new();

        for coedge_id in lp.coedges() {
            let coedge_key = coedge_id.as_str();
            if !seen.insert(coedge_key) {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:duplicate-coedge:{coedge_id}"),
                    StepLossCode::LoopDuplicateCoedge,
                    format!(
                        "loop {loop_id} cannot be ordered because coedge {coedge_id} occurs more than once"
                    ),
                );
                return None;
            }
            let Some(coedge) = self.coedges.get(coedge_key).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:coedge:{coedge_id}"),
                    StepLossCode::LoopCoedgeMissingForOrder,
                    format!(
                        "loop {loop_id} cannot be ordered because coedge {coedge_id} is missing"
                    ),
                );
                return None;
            };
            let edge_key = coedge.edge.as_str();
            let Some(edge) = self.edges.get(edge_key).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:edge:{edge_key}"),
                    StepLossCode::LoopEdgeMissingForOrder,
                    format!("loop {loop_id} cannot be ordered because edge {edge_key} is missing"),
                );
                return None;
            };
            let Some(curve_id) = edge.curve() else {
                self.curveless_edges.insert(edge_key.to_string());
                self.topology_relation_loss(
                    format!("edge:{edge_key}:curve"),
                    StepLossCode::EdgeNo3dCurve,
                    format!("edge {edge_key} was omitted because it has no 3D curve"),
                );
                continue;
            };
            let Some(curve) = self.curves.get(curve_id.as_str()).copied() else {
                self.topology_relation_loss(
                    format!("edge:{edge_key}:curve:{curve_id}"),
                    StepLossCode::EdgeCurveNotWritable,
                    format!("edge {edge_key} was omitted because curve {curve_id} is not writable"),
                );
                continue;
            };
            if !geometry::curve_is_supported(&curve.geometry) {
                self.curveless_edges.insert(edge_key.to_string());
                self.topology_relation_loss(
                    format!("edge:{edge_key}:unsupported-curve:{curve_id}"),
                    StepLossCode::EdgeCurveUnsupported,
                    format!("edge {edge_key} was omitted because curve {curve_id} is unsupported"),
                );
                continue;
            }
            let (start_vertex, end_vertex) = match coedge.sense {
                Sense::Forward => (&edge.start, &edge.end),
                Sense::Reversed => (&edge.end, &edge.start),
            };
            segments.push(LoopSegment {
                coedge_id: coedge_id.as_str().to_owned(),
                start_vertex: start_vertex.as_str().to_owned(),
                end_vertex: end_vertex.as_str().to_owned(),
            });
        }

        let mut outgoing: HashMap<String, Vec<usize>> = HashMap::new();
        let mut incoming: HashMap<&str, usize> = HashMap::new();
        for (index, segment) in segments.iter().enumerate() {
            outgoing
                .entry(segment.start_vertex.clone())
                .or_default()
                .push(index);
            *incoming.entry(segment.end_vertex.as_str()).or_default() += 1;
        }
        if segments.iter().any(|segment| {
            outgoing.get(&segment.start_vertex).map_or(0, Vec::len)
                != incoming
                    .get(segment.start_vertex.as_str())
                    .copied()
                    .unwrap_or(0)
        }) {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        // The IR coedge list and its next/previous links are not sufficient to
        // establish the STEP path order. Build an Eulerian circuit from the
        // oriented edge endpoints instead; this also handles repeated vertices
        // without choosing a geometric edge by position in the source list.
        for candidates in outgoing.values_mut() {
            candidates.reverse();
        }
        let first_start = segments.first()?.start_vertex.clone();
        let mut vertex_stack = vec![first_start.clone()];
        let mut edge_stack = Vec::new();
        let mut circuit = Vec::new();
        while let Some(vertex) = vertex_stack.last().cloned() {
            let next_edge = outgoing.get_mut(&vertex).and_then(Vec::pop);
            if let Some(edge_index) = next_edge {
                vertex_stack.push(segments[edge_index].end_vertex.clone());
                edge_stack.push(edge_index);
            } else {
                vertex_stack.pop();
                if let Some(edge_index) = edge_stack.pop() {
                    circuit.push(edge_index);
                }
            }
        }
        circuit.reverse();

        if circuit.len() != segments.len() {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        if let Some(first_index) = circuit.iter().position(|index| *index == 0) {
            circuit.rotate_left(first_index);
        }
        let mut current_vertex = segments.first()?.start_vertex.as_str();
        for edge_index in &circuit {
            let segment = &segments[*edge_index];
            if segment.start_vertex != current_vertex {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:continuity"),
                    StepLossCode::LoopNoContinuousOrdering,
                    format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
                );
                return None;
            }
            current_vertex = segment.end_vertex.as_str();
        }
        if current_vertex != first_start {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        Some(
            circuit
                .into_iter()
                .map(|index| segments[index].coedge_id.clone())
                .collect(),
        )
    }

    fn emit_edge(&mut self, edge_id: &str) -> Option<Ref> {
        if let Some(r) = self.edge_refs.get(edge_id) {
            return Some(*r);
        }
        let Some(edge) = self.edges.get(edge_id).copied() else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:record"),
                StepLossCode::EdgeRecordMissing,
                format!("edge {edge_id} was omitted because its record is missing"),
            );
            return None;
        };
        let Some(v1) = self.emit_vertex(edge.start.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:start-vertex:{}", edge.start),
                StepLossCode::EdgeStartVertexMissing,
                format!(
                    "edge {edge_id} was omitted because start vertex {} is missing",
                    edge.start
                ),
            );
            return None;
        };
        let Some(v2) = self.emit_vertex(edge.end.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:end-vertex:{}", edge.end),
                StepLossCode::EdgeEndVertexMissing,
                format!(
                    "edge {edge_id} was omitted because end vertex {} is missing",
                    edge.end
                ),
            );
            return None;
        };
        let Some(curve_id) = edge.curve() else {
            self.curveless_edges.insert(edge_id.to_string());
            self.topology_relation_loss(
                format!("edge:{edge_id}:curve"),
                StepLossCode::EdgeNo3dCurve,
                format!("edge {edge_id} was omitted because it has no 3D curve"),
            );
            return None;
        };
        if self
            .curves
            .get(curve_id.as_str())
            .is_some_and(|curve| !geometry::curve_is_supported(&curve.geometry))
        {
            self.curveless_edges.insert(edge_id.to_string());
            self.topology_relation_loss(
                format!("edge:{edge_id}:unsupported-curve:{curve_id}"),
                StepLossCode::EdgeCurveUnsupported,
                format!("edge {edge_id} was omitted because curve {curve_id} is unsupported"),
            );
            return None;
        }
        let Some(basis_curve) = self.emit_curve(curve_id.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:curve:{curve_id}"),
                StepLossCode::EdgeCurveNotWritable,
                format!("edge {edge_id} was omitted because curve {curve_id} is not writable"),
            );
            return None;
        };
        let associated = self.edge_coedges.get(edge_id).cloned().unwrap_or_default();
        let mut pcurve_refs = Vec::new();
        for (pcurve_id, surface_id, face_id) in associated {
            if let Some(pcurve) = self.emit_pcurve(pcurve_id, surface_id) {
                pcurve_refs.push(pcurve);
            } else if self.surface_chart_reversed(surface_id) {
                self.loss(
                    StepLossCode::PcurveCarrierUnwritable,
                    format!(
                        "face {face_id} pcurve {pcurve_id} was omitted because its geometry or surface reference could not be mapped to the reversed cone chart"
                    ),
                );
            } else if self.pcurves.contains_key(pcurve_id) {
                self.unwritten_pcurve_carriers.insert(pcurve_id.to_string());
            }
        }
        let curve_ref = if pcurve_refs.is_empty() {
            basis_curve
        } else {
            self.emitter.emit(
                "SURFACE_CURVE",
                &format!("'',{basis_curve},{},.CURVE_3D.", refs(&pcurve_refs)),
            )
        };
        // same_sense = .T.: the edge runs start→end along the curve's own
        // parameterization, the convention IR curves follow.
        let r = self
            .emitter
            .emit("EDGE_CURVE", &format!("'',{v1},{v2},{curve_ref},.T."));
        self.edge_refs.insert(edge_id.to_string(), r);
        Some(r)
    }

    fn emit_pcurve(&mut self, pcurve_id: &str, surface_id: &str) -> Option<Ref> {
        let pcurve = self.pcurves.get(pcurve_id).copied()?;
        let surface = self.emit_surface(surface_id)?;
        let reversed_cone = self.surface_chart_reversed(surface_id);
        let curve = if reversed_cone {
            geometry::pcurve_on_reversed_cone(&mut self.emitter, &pcurve.geometry)?
        } else {
            geometry::pcurve(&mut self.emitter, &pcurve.geometry)?
        };
        let context = if let Some(context) = self.pcurve_context {
            context
        } else {
            let context = self.emitter.emit_raw(
                "GEOMETRIC_REPRESENTATION_CONTEXT",
                "( GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D') )",
            );
            self.pcurve_context = Some(context);
            context
        };
        let representation = self.emitter.emit(
            "DEFINITIONAL_REPRESENTATION",
            &format!("'',({curve}),{context}"),
        );
        Some(
            self.emitter
                .emit("PCURVE", &format!("'',{surface},{representation}")),
        )
    }

    fn emit_vertex(&mut self, vertex_id: &str) -> Option<Ref> {
        if let Some(r) = self.vertex_refs.get(vertex_id) {
            return Some(*r);
        }
        let vertex = self.vertices.get(vertex_id).copied()?;
        let pt = self.points.get(vertex.point.as_str()).copied()?;
        let cp = geometry::point(&mut self.emitter, pt.position().get());
        self.point_refs.insert(vertex.point.as_str().to_owned(), cp);
        let r = self.emitter.emit("VERTEX_POINT", &format!("'',{cp}"));
        self.vertex_refs.insert(vertex_id.to_string(), r);
        Some(r)
    }

    fn emit_surface(&mut self, surface_id: &str) -> Option<Ref> {
        if let Some(written) = self.surface_refs.get(surface_id) {
            return Some(written.reference);
        }
        if self.geometry_emission_depth >= 256
            || !self.active_surfaces.insert(surface_id.to_string())
        {
            return None;
        }
        self.geometry_emission_depth += 1;
        let result = (|| {
            let surf = self.surfaces.get(surface_id).copied()?;
            let procedural = self.procedural_surfaces.get(surface_id).map(|procedural| {
                (
                    procedural.id.as_str().to_owned(),
                    procedural.definition().clone(),
                )
            });
            let solved = surf.geometry.solved();
            let emitted = procedural.and_then(|(id, definition)| {
                let reference = self.emit_procedural_surface(solved, &definition)?;
                let reversed = match &definition {
                    ProceduralSurfaceDefinition::ParallelOffset(payload) => {
                        self.surface_chart_reversed(payload.support().as_str())
                    }
                    // The emitted trim maps its support ranges; its own local
                    // chart remains unchanged.
                    ProceduralSurfaceDefinition::Subset(_) => false,
                    ProceduralSurfaceDefinition::Replica { source, .. } => {
                        self.surface_chart_reversed(source.as_str())
                    }
                    _ => false,
                };
                Some((id, reference, reversed))
            });
            let (r, reversed) = if let Some((id, reference, reversed)) = emitted {
                self.written_procedural_surfaces.insert(id);
                (reference, reversed)
            } else {
                let solved = solved?;
                if !geometry::surface_is_supported(solved) {
                    return None;
                }
                // The basis is taken before the record is emitted, so the
                // census cannot miss a record the writer put in the file.
                let basis = geometry::emitted_basis(solved);
                let reference = geometry::surface(&mut self.emitter, solved)?;
                self.written_analytic_surfaces
                    .push(WrittenAnalyticSurface::Carrier(basis));
                let reversed = matches!(basis, SolvedSurfaceGeometry::Cone(cone) if cone.half_angle().get() < 0.0);
                (reference, reversed)
            };
            Some((r, reversed))
        })();
        self.active_surfaces.remove(surface_id);
        self.geometry_emission_depth -= 1;
        if let Some((r, reversed)) = result {
            self.surface_refs.insert(
                surface_id.to_string(),
                WrittenSurface {
                    reference: r,
                    reversed_chart: reversed,
                },
            );
            return Some(r);
        }
        None
    }

    fn surface_chart_reversed(&self, surface_id: &str) -> bool {
        self.surface_refs
            .get(surface_id)
            .is_some_and(|surface| surface.reversed_chart)
    }

    fn emit_procedural_surface(
        &mut self,
        solved: Option<&'a SolvedSurfaceGeometry>,
        definition: &ProceduralSurfaceDefinition,
    ) -> Option<Ref> {
        let logical = |value: Option<bool>| match value {
            Some(true) => ".T.",
            Some(false) => ".F.",
            None => ".U.",
        };
        match definition {
            ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
                let direction = definition_payload.direction();
                let directrix = self.emit_curve(definition_payload.directrix().as_str())?;
                let direction_ref = geometry::direction(&mut self.emitter, *direction);
                let vector = self.emitter.emit(
                    "VECTOR",
                    &format!("'',{direction_ref},{}", self.emitter.real(direction.norm())),
                );
                Some(self.emitter.emit(
                    "SURFACE_OF_LINEAR_EXTRUSION",
                    &format!("'',{directrix},{vector}"),
                ))
            }
            ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
                let directrix = self.emit_curve(definition_payload.directrix().as_str())?;
                let origin =
                    geometry::point(&mut self.emitter, definition_payload.axis_origin().get());
                let direction = geometry::direction(
                    &mut self.emitter,
                    definition_payload.axis_direction().into(),
                );
                let axis = self
                    .emitter
                    .emit("AXIS1_PLACEMENT", &format!("'',{origin},{direction}"));
                Some(
                    self.emitter
                        .emit("SURFACE_OF_REVOLUTION", &format!("'',{directrix},{axis}")),
                )
            }
            ProceduralSurfaceDefinition::ParallelOffset(definition_payload) => {
                let support = definition_payload.support();
                let distance = definition_payload.distance();
                let self_intersect = definition_payload.self_intersect();
                {
                    let support = self.emit_surface(support.as_str())?;
                    Some(self.emitter.emit(
                        "OFFSET_SURFACE",
                        &format!(
                            "'',{support},{},{}",
                            self.emitter.real(distance.get()),
                            logical(*self_intersect)
                        ),
                    ))
                }
            }
            ProceduralSurfaceDefinition::Subset(payload) => {
                let support = payload.support();
                let parameter_ranges = payload
                    .parameter_ranges()
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints);
                let u_sense = payload.u_sense().as_ref()?;
                let v_sense = payload.v_sense().as_ref()?;
                let support = self.emit_surface(support.as_str())?;
                let reverse_chart = self.surface_chart_reversed(payload.support().as_str());
                Some(self.emit_rectangular_trim(
                    support,
                    parameter_ranges,
                    [*u_sense, *v_sense],
                    reverse_chart,
                ))
            }
            ProceduralSurfaceDefinition::Replica { source, transform } => {
                let source = self.emit_surface(source.as_str())?;
                let operator = geometry::transformation_operator(&mut self.emitter, *transform);
                Some(
                    self.emitter
                        .emit("SURFACE_REPLICA", &format!("'',{source},{operator}")),
                )
            }
            ProceduralSurfaceDefinition::DegenerateTorus { select_outer } => {
                let Some(SolvedSurfaceGeometry::Torus(torus_surface)) = solved else {
                    return None;
                };
                let center = torus_surface.center().get();
                let axis = *torus_surface.frame().axis();
                let ref_direction = *torus_surface.frame().reference();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                let placement = geometry::placement(&mut self.emitter, center, axis, ref_direction);
                let reference = self.emitter.emit(
                    "DEGENERATE_TOROIDAL_SURFACE",
                    &format!(
                        "'',{placement},{},{},{}",
                        self.emitter.real(major_radius.abs()),
                        self.emitter.real(minor_radius.abs()),
                        if *select_outer { ".T." } else { ".F." }
                    ),
                );
                self.written_analytic_surfaces
                    .push(WrittenAnalyticSurface::DegenerateTorus(torus_surface));
                Some(reference)
            }
            _ => None,
        }
    }

    fn emit_rectangular_trim(
        &mut self,
        support: Ref,
        mut ranges: [[f64; 2]; 2],
        senses: [bool; 2],
        reverse_chart: bool,
    ) -> Ref {
        if reverse_chart {
            for range in &mut ranges {
                range[0] = -range[0];
                range[1] = -range[1];
            }
        }
        self.emitter.emit(
            "RECTANGULAR_TRIMMED_SURFACE",
            &format!(
                "'',{support},{},{},{},{},{},{}",
                self.emitter.real(ranges[0][0]),
                self.emitter.real(ranges[0][1]),
                self.emitter.real(ranges[1][0]),
                self.emitter.real(ranges[1][1]),
                if senses[0] == reverse_chart {
                    ".F."
                } else {
                    ".T."
                },
                if senses[1] == reverse_chart {
                    ".F."
                } else {
                    ".T."
                },
            ),
        )
    }

    pub(crate) fn emit_curve(&mut self, curve_id: &str) -> Option<Ref> {
        if let Some(r) = self.curve_refs.get(curve_id) {
            return Some(*r);
        }
        if self.geometry_emission_depth >= 256 || !self.active_curves.insert(curve_id.to_string()) {
            return None;
        }
        self.geometry_emission_depth += 1;
        let result = (|| {
            let geometry = self.curves.get(curve_id)?.geometry.clone();
            let procedural = self.procedural_curves.get(curve_id).map(|procedural| {
                (
                    procedural.id.as_str().to_owned(),
                    procedural.definition().clone(),
                )
            });
            let emitted = procedural.and_then(|(id, definition)| {
                self.emit_procedural_curve(&definition)
                    .map(|reference| (id, reference))
            });
            let r = if let Some((id, reference)) = emitted {
                self.written_procedural_curves.insert(id);
                reference
            } else if let CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments,
                self_intersect,
            }) = &geometry
            {
                let mut segment_refs = Vec::new();
                for segment in segments {
                    let curve = self.emit_curve(segment.curve.as_str())?;
                    let transition = match segment.transition {
                    cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous => {
                        ".DISCONTINUOUS."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::Continuous => ".CONTINUOUS.",
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient => {
                        ".CONTSAMEGRADIENT."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradientSameCurvature => {
                        ".CONTSAMEGRADIENTSAMECURVATURE."
                    }
                };
                    segment_refs.push(self.emitter.emit(
                        "COMPOSITE_CURVE_SEGMENT",
                        &format!(
                            "{transition},{},{curve}",
                            if segment.same_sense { ".T." } else { ".F." }
                        ),
                    ));
                }
                self.emitter.emit(
                    "COMPOSITE_CURVE",
                    &format!(
                        "'',{},{}",
                        refs(&segment_refs),
                        match self_intersect {
                            Some(true) => ".T.",
                            Some(false) => ".F.",
                            None => ".U.",
                        }
                    ),
                )
            } else if !geometry::curve_is_supported(&geometry) {
                return None;
            } else {
                geometry::curve(&mut self.emitter, geometry.solved()?)?
            };
            Some(r)
        })();
        self.active_curves.remove(curve_id);
        self.geometry_emission_depth -= 1;
        if let Some(r) = result {
            self.curve_refs.insert(curve_id.to_string(), r);
        }
        result
    }

    fn emit_procedural_curve(&mut self, definition: &ProceduralCurveDefinition) -> Option<Ref> {
        match definition {
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let [start, end] = definition_payload.parameter_range().endpoints();
                let sense = definition_payload.sense();
                {
                    let source = self.emit_curve(source.as_str())?;
                    let (start, end) = if *sense { (start, end) } else { (end, start) };
                    Some(self.emitter.emit(
                        "TRIMMED_CURVE",
                        &format!(
                        "'',{source},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),{},.PARAMETER.",
                        self.emitter.real(start),
                        self.emitter.real(end),
                        if *sense { ".T." } else { ".F." }
                    ),
                    ))
                }
            }
            ProceduralCurveDefinition::Replica { source, transform } => {
                let source = self.emit_curve(source.as_str())?;
                let operator = geometry::transformation_operator(&mut self.emitter, *transform);
                Some(
                    self.emitter
                        .emit("CURVE_REPLICA", &format!("'',{source},{operator}")),
                )
            }
            ProceduralCurveDefinition::SpatialOffset(definition_payload) => {
                let source = definition_payload.source();
                let distance = definition_payload.distance();
                let reference_direction = definition_payload.reference_direction();
                let self_intersect = definition_payload.self_intersect();
                {
                    let source = self.emit_curve(source.as_str())?;
                    let direction =
                        geometry::direction(&mut self.emitter, (*reference_direction).into());
                    let self_intersect = match self_intersect {
                        Some(true) => ".T.",
                        Some(false) => ".F.",
                        None => ".U.",
                    };
                    Some(self.emitter.emit(
                        "OFFSET_CURVE_3D",
                        &format!(
                            "'',{source},{},{self_intersect},{direction}",
                            self.emitter.real(distance.get())
                        ),
                    ))
                }
            }
            _ => None,
        }
    }

    fn pmi_target_ref(&self, target: &PmiTarget) -> Option<Ref> {
        match target {
            PmiTarget::Body { body } => self
                .body_step_refs
                .get(body.as_str())
                .copied()
                .or_else(|| self.body_shape_refs.get(body.as_str()).copied()),
            PmiTarget::Face { face } => self.face_step_refs.get(face.as_str()).copied(),
            PmiTarget::Edge { edge } => self.edge_refs.get(edge.as_str()).copied(),
            PmiTarget::Vertex { vertex } => self.vertex_refs.get(vertex.as_str()).copied(),
            PmiTarget::Point { point } => self.point_refs.get(point.as_str()).copied(),
            PmiTarget::Curve { curve } => self.curve_refs.get(curve.as_str()).copied(),
            PmiTarget::Product { .. }
            | PmiTarget::Occurrence { .. }
            | PmiTarget::ShapeAspect { .. } => None,
        }
    }

    fn emit_datum_target(
        &mut self,
        pds: Ref,
        annotation: &cadmpeg_ir::PmiAnnotation,
        form: &DatumTargetForm,
        identification: &str,
    ) -> Ref {
        self.emitter.emit(
            "DATUM_TARGET",
            &format!(
                "{},{},{pds},.F.,{}",
                string(annotation.name.as_deref().unwrap_or("")),
                string(datum_target_form_text(form)),
                string(identification)
            ),
        )
    }

    fn emit_pmi(&mut self, context: Ref) {
        if self.ir.model.pmi.is_empty() || !self.schema.supports_semantic_pmi() {
            return;
        }
        let annotations = self.ir.model.pmi.clone();
        let Some(pds) = self.default_product_definition_shape else {
            return;
        };
        let mut annotation_refs = HashMap::new();
        let mut aspects = HashMap::<String, Ref>::new();
        for annotation in &annotations {
            let PmiDefinition::DatumTarget {
                form,
                identification,
                ..
            } = &annotation.definition
            else {
                continue;
            };
            for target in &annotation.targets {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    continue;
                };
                if aspects.contains_key(source_id.as_str()) {
                    continue;
                }
                let target = self.emit_datum_target(pds, annotation, form, identification);
                aspects.insert(source_id.to_string(), target);
            }
        }
        for annotation in &annotations {
            for target in &annotation.targets {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    continue;
                };
                aspects.entry(source_id.to_string()).or_insert_with(|| {
                    self.emitter.emit(
                        "SHAPE_ASPECT",
                        &format!("{},'',{pds},.T.", string(source_id.as_str())),
                    )
                });
            }
        }
        for annotation in &annotations {
            let PmiDefinition::DatumTarget { basis, .. } = &annotation.definition else {
                continue;
            };
            for target in basis {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    continue;
                };
                aspects.entry(source_id.to_string()).or_insert_with(|| {
                    self.emitter.emit(
                        "SHAPE_ASPECT",
                        &format!("{},'',{pds},.T.", string(source_id.as_str())),
                    )
                });
            }
        }
        let mut datum_target_refs = HashMap::<cadmpeg_ir::ids::PmiId, Ref>::new();
        for annotation in &annotations {
            let PmiDefinition::DatumTarget {
                form,
                identification,
                ..
            } = &annotation.definition
            else {
                continue;
            };
            let target_ref = annotation.targets.iter().find_map(|target| {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    return None;
                };
                aspects.get(source_id.as_str()).copied()
            });
            let target_ref = target_ref
                .unwrap_or_else(|| self.emit_datum_target(pds, annotation, form, identification));
            datum_target_refs.insert(annotation.id.clone(), target_ref);
        }
        let mut datum_target_basis_exact = HashMap::<cadmpeg_ir::ids::PmiId, bool>::new();
        for annotation in &annotations {
            let PmiDefinition::DatumTarget { basis, .. } = &annotation.definition else {
                continue;
            };
            let Some(&datum_target) = datum_target_refs.get(&annotation.id) else {
                datum_target_basis_exact.insert(annotation.id.clone(), false);
                continue;
            };
            let mut exact = true;
            for target in basis {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    exact = false;
                    continue;
                };
                let Some(basis_ref) = aspects.get(source_id.as_str()).copied() else {
                    exact = false;
                    continue;
                };
                self.emitter.emit(
                    "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP",
                    &format!("'','',{basis_ref},{datum_target}"),
                );
            }
            datum_target_basis_exact.insert(annotation.id.clone(), exact);
        }
        let fallback_aspect = self
            .emitter
            .emit("SHAPE_ASPECT", &format!("'PMI target','',{pds},.T."));
        let target_ref = |annotation: &cadmpeg_ir::PmiAnnotation| {
            annotation
                .targets
                .iter()
                .find_map(|target| {
                    if let PmiTarget::ShapeAspect { source_id } = target {
                        aspects.get(source_id.as_str()).copied()
                    } else {
                        None
                    }
                })
                .or_else(|| datum_target_refs.get(&annotation.id).copied())
        };
        let mut target_items = Vec::new();
        for annotation in &annotations {
            if matches!(&annotation.definition, PmiDefinition::Presentation { .. }) {
                continue;
            }
            for target in &annotation.targets {
                if let Some(target_ref) = self.pmi_target_ref(target) {
                    if !target_items.contains(&target_ref) {
                        target_items.push(target_ref);
                    }
                }
            }
        }
        let target_representation = (!target_items.is_empty()).then(|| {
            self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("'PMI geometric targets',{},{context}", refs(&target_items)),
            )
        });
        let mut targets_exact_by_annotation = HashMap::new();
        for annotation in &annotations {
            let semantic = !matches!(&annotation.definition, PmiDefinition::Presentation { .. });
            let definition = target_ref(annotation).unwrap_or(fallback_aspect);
            let mut exact = true;
            for target in &annotation.targets {
                let Some(identified_item) = self.pmi_target_ref(target) else {
                    if !matches!(target, PmiTarget::ShapeAspect { .. }) {
                        exact = false;
                    }
                    continue;
                };
                let Some(used_representation) = target_representation else {
                    exact = false;
                    continue;
                };
                if semantic {
                    self.emitter.emit(
                        "GEOMETRIC_ITEM_SPECIFIC_USAGE",
                        &format!("'','',{definition},{used_representation},{identified_item}"),
                    );
                } else {
                    exact = false;
                }
            }
            if matches!(&annotation.definition, PmiDefinition::DatumTarget { .. }) {
                exact &= datum_target_basis_exact
                    .get(&annotation.id)
                    .copied()
                    .unwrap_or(false);
            }
            targets_exact_by_annotation.insert(annotation.id.clone(), exact);
        }
        let targets_exact = |annotation: &cadmpeg_ir::PmiAnnotation| {
            targets_exact_by_annotation
                .get(&annotation.id)
                .copied()
                .unwrap_or(false)
        };

        for annotation in &annotations {
            if let PmiDefinition::Datum { identification } = &annotation.definition {
                let datum = self.emitter.emit(
                    "DATUM",
                    &format!(
                        "{},$,{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        string(identification)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), datum);
                self.written_pmi += usize::from(targets_exact(annotation));
            }
        }
        for annotation in &annotations {
            if matches!(&annotation.definition, PmiDefinition::DatumTarget { .. }) {
                self.written_pmi += usize::from(targets_exact(annotation));
            }
        }
        for annotation in &annotations {
            if let PmiDefinition::DatumSystem { references } = &annotation.definition {
                let mut groups = BTreeMap::<(u32, Option<u32>), Vec<_>>::new();
                for reference in references.as_slice() {
                    groups
                        .entry((reference.precedence.get(), reference.common_group))
                        .or_default()
                        .push(reference);
                }
                let compartments = groups
                    .values()
                    .filter_map(|group| {
                        let first = group.first()?;
                        let datum_refs = group
                            .iter()
                            .map(|reference| annotation_refs.get(&reference.datum).copied())
                            .collect::<Option<Vec<_>>>()?;
                        let (datum, modifiers) = if first.common_group.is_some() {
                            let elements = group
                                .iter()
                                .zip(datum_refs)
                                .map(|(reference, datum)| {
                                    let modifiers =
                                        self.emit_datum_modifiers(&reference.modifiers)?;
                                    Some(self.emitter.emit(
                                        "DATUM_REFERENCE_ELEMENT",
                                        &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                                    ))
                                })
                                .collect::<Option<Vec<_>>>()?;
                            (
                                format!("COMMON_DATUM_LIST({})", refs(&elements)),
                                String::new(),
                            )
                        } else {
                            (
                                datum_refs.first()?.to_string(),
                                self.emit_datum_modifiers(&first.modifiers)?,
                            )
                        };
                        Some(self.emitter.emit(
                            "DATUM_REFERENCE_COMPARTMENT",
                            &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                        ))
                    })
                    .collect::<Vec<_>>();
                let complete = compartments.len() == groups.len();
                if compartments.is_empty() {
                    continue;
                }
                let system = self.emitter.emit(
                    "DATUM_SYSTEM",
                    &format!(
                        "{},'',{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        refs(&compartments)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), system);
                self.written_pmi += usize::from(targets_exact(annotation) && complete);
            }
        }
        for annotation in &annotations {
            match &annotation.definition {
                PmiDefinition::Dimension(relation) => {
                    let dimension = relation.kind();
                    let nominal = relation.nominal();
                    let tolerance = relation.tolerance();
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    let name = annotation.name.as_deref().unwrap_or("");
                    let (entity, kind_exact) = match dimension {
                        DimensionKind::Size => ("DIMENSIONAL_SIZE", true),
                        DimensionKind::Location => ("DIMENSIONAL_LOCATION", true),
                        DimensionKind::Angular => ("ANGULAR_SIZE", true),
                        // AP242 represents diameter and radius as a
                        // DIMENSIONAL_SIZE whose name identifies the size
                        // category; DIAMETER_SIZE and RADIUS_SIZE are not
                        // entity types.
                        DimensionKind::Diameter | DimensionKind::Radius => {
                            ("DIMENSIONAL_SIZE", true)
                        }
                        DimensionKind::Other(_) => ("DIMENSIONAL_SIZE", false),
                    };
                    let characteristic_name = match dimension {
                        DimensionKind::Diameter => "diameter",
                        DimensionKind::Radius => "radius",
                        _ => name,
                    };
                    let parameters = match dimension {
                        DimensionKind::Location => {
                            format!("{},$,{aspect},{aspect}", string(characteristic_name))
                        }
                        DimensionKind::Angular => {
                            format!("{aspect},{},.SMALL.", string(characteristic_name))
                        }
                        _ => format!("{aspect},{}", string(characteristic_name)),
                    };
                    let characteristic = self.emitter.emit(entity, &parameters);
                    if let Some(value) = nominal {
                        let measure = self.emit_pmi_measure_representation_item(*value, name);
                        let representation = self.emitter.emit(
                            "SHAPE_DIMENSION_REPRESENTATION",
                            &format!("'',({measure}),{context}"),
                        );
                        self.emitter.emit(
                            "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION",
                            &format!("{characteristic},{representation}"),
                        );
                    }
                    if let Some(
                        DimensionTolerance::PlusMinus { lower, upper }
                        | DimensionTolerance::PlusMinusFit { lower, upper, .. },
                    ) = tolerance
                    {
                        let lower = self.emit_pmi_measure(*lower);
                        let upper = self.emit_pmi_measure(*upper);
                        let tolerance = self
                            .emitter
                            .emit("TOLERANCE_VALUE", &format!("{lower},{upper}"));
                        self.emitter.emit(
                            "PLUS_MINUS_TOLERANCE",
                            &format!("{tolerance},{characteristic}"),
                        );
                    }
                    if let Some(
                        DimensionTolerance::Fit { fit }
                        | DimensionTolerance::PlusMinusFit { fit, .. },
                    ) = tolerance
                    {
                        let fit = self.emitter.emit(
                            "LIMITS_AND_FITS",
                            &format!(
                                "{},{},{},{}",
                                string(&fit.form_variance),
                                string(&fit.zone_variance),
                                string(&fit.grade),
                                string(&fit.source)
                            ),
                        );
                        self.emitter
                            .emit("PLUS_MINUS_TOLERANCE", &format!("{fit},{characteristic}"));
                    }
                    annotation_refs.insert(annotation.id.clone(), characteristic);
                    self.written_pmi += usize::from(targets_exact(annotation) && kind_exact);
                }
                PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers,
                } => {
                    let kind_exact = !matches!(tolerance, GeometricToleranceKind::Other(value) if value != "geometric_tolerance");
                    let entity = match tolerance {
                        GeometricToleranceKind::Straightness => "STRAIGHTNESS_TOLERANCE",
                        GeometricToleranceKind::Flatness => "FLATNESS_TOLERANCE",
                        GeometricToleranceKind::Roundness => "ROUNDNESS_TOLERANCE",
                        GeometricToleranceKind::Cylindricity => "CYLINDRICITY_TOLERANCE",
                        GeometricToleranceKind::Coaxiality => "COAXIALITY_TOLERANCE",
                        GeometricToleranceKind::LineProfile => "LINE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::SurfaceProfile => "SURFACE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::Angularity => "ANGULARITY_TOLERANCE",
                        GeometricToleranceKind::Perpendicularity => "PERPENDICULARITY_TOLERANCE",
                        GeometricToleranceKind::Parallelism => "PARALLELISM_TOLERANCE",
                        GeometricToleranceKind::Position => "POSITION_TOLERANCE",
                        GeometricToleranceKind::Concentricity => "CONCENTRICITY_TOLERANCE",
                        GeometricToleranceKind::Symmetry => "SYMMETRY_TOLERANCE",
                        GeometricToleranceKind::CircularRunout => "CIRCULAR_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::TotalRunout => "TOTAL_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::Other(_) => continue,
                    };
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    let modifier_values = Self::emit_geometric_tolerance_modifiers(modifiers);
                    if !modifiers.is_empty() && modifier_values.is_none() {
                        continue;
                    }
                    let area_unit =
                        Self::emit_geometric_tolerance_area_unit(defined_area_unit.as_deref());
                    if defined_area_unit.is_some() && area_unit.is_none()
                        || defined_area_second_unit.is_some() && area_unit.is_none()
                    {
                        continue;
                    }
                    let datum_ref = datum_system
                        .as_ref()
                        .and_then(|id| annotation_refs.get(id).copied());
                    if datum_system.is_some() && datum_ref.is_none() {
                        continue;
                    }
                    let measure = self.emit_pmi_measure(magnitude.get());
                    let tolerance_ref = if datum_ref.is_none()
                        && modifiers.is_empty()
                        && defined_unit.is_none()
                        && area_unit.is_none()
                    {
                        self.emitter.emit(
                            entity,
                            &format!(
                                "{},'',{measure},{aspect}",
                                string(annotation.name.as_deref().unwrap_or(""))
                            ),
                        )
                    } else {
                        let mut parts = vec![format!(
                            "GEOMETRIC_TOLERANCE({},{},{measure},{aspect})",
                            string(annotation.name.as_deref().unwrap_or("")),
                            "''"
                        )];
                        if area_unit.is_some() || defined_unit.is_some() {
                            let defined_unit = defined_unit.as_ref().map_or_else(
                                || "$".into(),
                                |unit| self.emit_pmi_measure(*unit).to_string(),
                            );
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT({defined_unit})"
                            ));
                        }
                        if let Some(area_unit) = area_unit {
                            let second_unit = defined_area_second_unit.as_ref().map_or_else(
                                || "$".into(),
                                |unit| self.emit_pmi_measure(*unit).to_string(),
                            );
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT(.{area_unit}.,{second_unit})"
                            ));
                        }
                        if let Some(datum) = datum_ref {
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE(({datum}))"
                            ));
                        }
                        if let Some(modifiers) = modifier_values {
                            parts
                                .push(format!("GEOMETRIC_TOLERANCE_WITH_MODIFIERS(({modifiers}))"));
                        }
                        parts.push(format!("{entity}()"));
                        self.emitter
                            .emit_raw("GEOMETRIC_TOLERANCE", &format!("({})", parts.join(" ")))
                    };
                    annotation_refs.insert(annotation.id.clone(), tolerance_ref);
                    self.written_pmi += usize::from(targets_exact(annotation) && kind_exact);
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
                | PmiDefinition::DatumTarget { .. }
                | PmiDefinition::Presentation { .. } => {}
            }
        }
        let mut presentation_items = Vec::new();
        let mut presentation_semantics = Vec::new();
        let mut hidden_presentation_items = Vec::new();
        for annotation in &annotations {
            let PmiDefinition::Presentation {
                text,
                placement,
                semantics,
            } = &annotation.definition
            else {
                continue;
            };
            let (Some(text), Some(placement)) = (text.as_deref(), placement.as_ref()) else {
                continue;
            };
            if !annotation.targets.is_empty() {
                continue;
            }
            let Some(frame) = placement.proper_rigid_frame() else {
                continue;
            };
            let rows = placement.rows();
            let placement = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                *frame.axis(),
                *frame.reference(),
            );
            let font_source = self.emitter.emit("EXTERNAL_SOURCE", "'ISO 3098'");
            let font = self.emitter.emit(
                "EXTERNALLY_DEFINED_TEXT_FONT",
                &format!("IDENTIFIER('ISO 3098'),{font_source}"),
            );
            let literal = self.emitter.emit(
                "TEXT_LITERAL",
                &format!("{},{placement},'left',.RIGHT.,{font}", string(text)),
            );
            let semantic_refs = semantics
                .iter()
                .filter_map(|semantic| annotation_refs.get(semantic).copied())
                .collect::<Vec<_>>();
            if semantic_refs.len() != semantics.len() {
                continue;
            }
            let style = self
                .emitter
                .emit("PRESENTATION_STYLE_ASSIGNMENT", "(.NULL.)");
            let occurrence = self.emitter.emit(
                "ANNOTATION_TEXT_OCCURRENCE",
                &format!(
                    "{},{},{literal}",
                    string(annotation.name.as_deref().unwrap_or("")),
                    refs(&[style])
                ),
            );
            presentation_items.push(occurrence);
            if annotation.visible == Some(false) {
                hidden_presentation_items.push(occurrence);
            }
            presentation_semantics.push((occurrence, semantic_refs));
            annotation_refs.insert(annotation.id.clone(), occurrence);
            self.written_pmi += 1;
        }
        if !presentation_items.is_empty() {
            let model = self.emitter.emit(
                "DRAUGHTING_MODEL",
                &format!(
                    "'PMI presentation',{}, {context}",
                    refs(&presentation_items)
                ),
            );
            for (occurrence, semantics) in presentation_semantics {
                for semantic in semantics {
                    self.emitter.emit(
                        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
                        &format!("'','',{semantic},{model},{occurrence}"),
                    );
                }
            }
        }
        if !hidden_presentation_items.is_empty() {
            if self.schema.supports_visibility() {
                self.emitter
                    .emit("INVISIBILITY", &refs(&hidden_presentation_items));
            } else {
                self.loss(
                    StepLossCode::HiddenPmiVisibilityUnsupported,
                    format!(
                        "{} hidden PMI annotation visibility assignment(s) are unsupported \
                         by {}; this write target does not emit INVISIBILITY",
                        hidden_presentation_items.len(),
                        self.schema.file_schema()
                    ),
                );
            }
        }
        for (annotation, reference) in annotation_refs {
            self.pmi_step_refs
                .insert(annotation.into_string(), reference);
        }
    }

    fn emit_datum_modifiers(&mut self, source: &[String]) -> Option<String> {
        const SIMPLE: &[&str] = &[
            "free_state",
            "basic",
            "translation",
            "least_material_requirement",
            "maximum_material_requirement",
            "point",
            "line",
            "plane",
            "orientation",
            "any_cross_section",
            "any_longitudinal_section",
            "contacting_feature",
            "distance_variable",
            "degree_of_freedom_constraint_x",
            "degree_of_freedom_constraint_y",
            "degree_of_freedom_constraint_z",
            "degree_of_freedom_constraint_u",
            "degree_of_freedom_constraint_v",
            "degree_of_freedom_constraint_w",
            "minor_diameter",
            "major_diameter",
            "pitch_diameter",
        ];
        const WITH_VALUE: &[&str] = &[
            "circular_or_cylindrical",
            "spherical",
            "distance",
            "projected",
        ];
        enum Modifier {
            Simple(String),
            WithValue {
                kind: String,
                value: cadmpeg_ir::PmiValue,
            },
        }
        let parsed = source
            .iter()
            .map(|modifier| {
                if let Some((kind, value)) = modifier.split_once(':') {
                    let kind = kind.to_ascii_lowercase();
                    let value = cadmpeg_ir::PmiValue::new(
                        value.parse::<f64>().ok()?,
                        cadmpeg_ir::PmiQuantity::Length,
                    )?;
                    WITH_VALUE
                        .contains(&kind.as_str())
                        .then_some(Modifier::WithValue { kind, value })
                } else {
                    let modifier = modifier.to_ascii_lowercase();
                    SIMPLE
                        .contains(&modifier.as_str())
                        .then_some(Modifier::Simple(modifier))
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let mut modifiers = Vec::new();
        for modifier in parsed {
            match modifier {
                Modifier::WithValue { kind, value } => {
                    let measure = self.emit_pmi_measure(value);
                    modifiers.push(
                        self.emitter
                            .emit(
                                "DATUM_REFERENCE_MODIFIER_WITH_VALUE",
                                &format!(".{}.,{measure}", kind.to_ascii_uppercase()),
                            )
                            .to_string(),
                    );
                }
                Modifier::Simple(modifier) => {
                    modifiers.push(format!(".{}.", modifier.to_ascii_uppercase()));
                }
            }
        }
        Some(modifiers.join(","))
    }

    fn emit_geometric_tolerance_modifiers(source: &[String]) -> Option<String> {
        const SUPPORTED: &[&str] = &[
            "any_cross_section",
            "associated_least_square_feature",
            "associated_maximum_inscribed_feature",
            "associated_minimum_inscribed_feature",
            "associated_minmax_feature",
            "associated_tangent_feature",
            "circle_a",
            "common_zone",
            "continuous_features",
            "derived_feature",
            "each_element",
            "each_radial_element",
            "free_state",
            "individually",
            "least_material_requirement",
            "line_element",
            "major_diameter",
            "maximum_material_requirement",
            "minor_diameter",
            "not_convex",
            "offset_zone",
            "peak_height",
            "pitch_diameter",
            "reciprocity_requirement",
            "reference_least_square_feature_with_external_material_constraint",
            "reference_least_square_feature_with_internal_material_constraint",
            "reference_least_square_feature_without_constraint",
            "reference_maximum_inscribed_feature",
            "reference_minimax_feature_with_external_material_constraint",
            "reference_minimax_feature_with_internal_material_constraint",
            "reference_minimax_feature_without_constraint",
            "reference_minimum_circumscribed_feature",
            "separate_requirement",
            "separate_zones",
            "standard_deviation",
            "statistical_tolerance",
            "stock",
            "tangent_plane",
            "total_range_deviations",
            "united_feature",
            "unspecified_angular_tolerance_zone_offset",
            "unspecified_linear_tolerance_zone_offset",
            "valley_depth",
            "variable_angle",
        ];
        source
            .iter()
            .map(|modifier| {
                let normalized = modifier.to_ascii_lowercase();
                SUPPORTED
                    .contains(&normalized.as_str())
                    .then(|| format!(".{}.", normalized.to_ascii_uppercase()))
            })
            .collect::<Option<Vec<_>>>()
            .map(|modifiers| modifiers.join(","))
    }

    fn emit_geometric_tolerance_area_unit(source: Option<&str>) -> Option<String> {
        let source = source?;
        matches!(
            source.to_ascii_lowercase().as_str(),
            "circular" | "square" | "rectangular" | "cylindrical" | "spherical"
        )
        .then(|| source.to_ascii_lowercase())
    }

    fn emit_pmi_measure(&mut self, value: cadmpeg_ir::PmiValue) -> Ref {
        let (entity, typed, unit) = match value.quantity {
            PmiQuantity::Length => (
                "LENGTH_MEASURE_WITH_UNIT",
                "LENGTH_MEASURE",
                self.emit_length_unit(),
            ),
            PmiQuantity::Angle => (
                "PLANE_ANGLE_MEASURE_WITH_UNIT",
                "PLANE_ANGLE_MEASURE",
                self.emit_angle_unit(),
            ),
            PmiQuantity::Ratio => ("MEASURE_WITH_UNIT", "RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter.emit(
            entity,
            &format!("{typed}({}),{unit}", self.emitter.real(value.value.get())),
        )
    }

    fn emit_pmi_measure_representation_item(
        &mut self,
        value: cadmpeg_ir::PmiValue,
        name: &str,
    ) -> Ref {
        let (typed, unit) = match value.quantity {
            PmiQuantity::Length => ("LENGTH_MEASURE", self.emit_length_unit()),
            PmiQuantity::Angle => ("PLANE_ANGLE_MEASURE", self.emit_angle_unit()),
            PmiQuantity::Ratio => ("RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter.emit(
            "MEASURE_REPRESENTATION_ITEM",
            &format!(
                "{},{typed}({}),{unit}",
                string(name),
                self.emitter.real(value.value.get())
            ),
        )
    }

    fn note_unrepresented(&mut self) {
        let mut referenced_shells = BTreeSet::new();
        let mut referenced_faces = BTreeSet::new();
        let mut referenced_loops = BTreeSet::new();
        let mut referenced_coedges = BTreeSet::new();
        let mut referenced_edges = BTreeSet::new();
        let mut referenced_vertices = BTreeSet::new();
        for region in &self.ir.model.regions {
            for shell_id in &region.shells {
                if !referenced_shells.insert(shell_id.as_str()) {
                    continue;
                }
                let Some(shell) = self.shells.get(shell_id.as_str()).copied() else {
                    continue;
                };
                for edge_id in shell.wire_edges() {
                    referenced_edges.insert(edge_id.as_str());
                    if let Some(edge) = self.edges.get(edge_id.as_str()).copied() {
                        referenced_vertices.insert(edge.start.as_str());
                        referenced_vertices.insert(edge.end.as_str());
                    }
                }
                referenced_vertices.extend(
                    shell
                        .free_vertices()
                        .iter()
                        .map(cadmpeg_ir::ids::VertexId::as_str),
                );
                for face_id in shell.faces() {
                    if !referenced_faces.insert(face_id.as_str()) {
                        continue;
                    }
                    let Some(face) = self.faces.get(face_id.as_str()).copied() else {
                        continue;
                    };
                    for loop_id in &face.loops {
                        if !referenced_loops.insert(loop_id.as_str()) {
                            continue;
                        }
                        let Some(loop_) = self.loops.get(loop_id.as_str()).copied() else {
                            continue;
                        };
                        referenced_vertices
                            .extend(loop_.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
                        for vertex_use in loop_.anchored_vertex_uses() {
                            referenced_coedges.insert(vertex_use.after.as_str());
                        }
                        for coedge_id in loop_.coedges() {
                            if !referenced_coedges.insert(coedge_id.as_str()) {
                                continue;
                            }
                            let Some(coedge) = self.coedges.get(coedge_id.as_str()).copied() else {
                                continue;
                            };
                            referenced_edges.insert(coedge.edge.as_str());
                            if let Some(edge) = self.edges.get(coedge.edge.as_str()).copied() {
                                referenced_vertices.insert(edge.start.as_str());
                                referenced_vertices.insert(edge.end.as_str());
                            }
                        }
                    }
                }
            }
        }
        let omitted_bodies = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| !self.body_shape_refs.contains_key(body.id.as_str()))
            .count();
        let omitted_shells = self
            .ir
            .model
            .shells
            .iter()
            .filter(|shell| !referenced_shells.contains(shell.id.as_str()))
            .count();
        let omitted_faces = self
            .ir
            .model
            .faces
            .iter()
            .filter(|face| !referenced_faces.contains(face.id.as_str()))
            .count();
        let omitted_loops = self
            .ir
            .model
            .loops
            .iter()
            .filter(|loop_| !referenced_loops.contains(loop_.id.as_str()))
            .count();
        let omitted_coedges = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !referenced_coedges.contains(coedge.id.as_str()))
            .count();
        let omitted_edges = self
            .ir
            .model
            .edges
            .iter()
            .filter(|edge| !referenced_edges.contains(edge.id.as_str()))
            .count();
        let omitted_vertices = self
            .ir
            .model
            .vertices
            .iter()
            .filter(|vertex| !referenced_vertices.contains(vertex.id.as_str()))
            .count();
        let omitted_topology = [
            ("body", omitted_bodies),
            ("shell", omitted_shells),
            ("face", omitted_faces),
            ("loop", omitted_loops),
            ("coedge", omitted_coedges),
            ("edge", omitted_edges),
            ("vertex", omitted_vertices),
        ];
        if omitted_topology.iter().any(|(_, count)| *count > 0) {
            let details = omitted_topology
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{count} {kind}(s)"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::TopologyUnreachableFromRegion,
                format!("topology not reachable from any emitted region shape item: {details}"),
            );
        }
        let nonstandard_analytic_surfaces = self
            .written_analytic_surfaces
            .iter()
            .filter(|written| match written {
                WrittenAnalyticSurface::Carrier(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                    let radius = sphere_surface.radius().get();
                    radius < 0.0
                }
                WrittenAnalyticSurface::Carrier(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    let major_radius = torus_surface.major_radius().get();
                    let minor_radius = torus_surface.minor_radius().get();
                    minor_radius < 0.0 || minor_radius.abs() > major_radius.abs()
                }
                // `DEGENERATE_TOROIDAL_SURFACE` holds a tube radius larger than
                // the major radius, so only the tube radius sign is normalized.
                WrittenAnalyticSurface::DegenerateTorus(torus_surface) => {
                    torus_surface.minor_radius().get() < 0.0
                }
                WrittenAnalyticSurface::Carrier(_) => false,
            })
            .count();
        if nonstandard_analytic_surfaces > 0 {
            self.loss(
                StepLossCode::AnalyticSurfaceNormalized,
                format!(
                    "{nonstandard_analytic_surfaces} signed or self-intersecting analytic \
                     surface(s) were normalized to positive STEP radii"
                ),
            );
        }
        let elliptical_cones = self
            .written_analytic_surfaces
            .iter()
            .filter(|written| {
                matches!(written, WrittenAnalyticSurface::Carrier(SolvedSurfaceGeometry::Cone(cone_surface))
                if {
                    let ratio = cone_surface.ratio().get();
                    ratio != 1.0
                })
            })
            .count();
        if elliptical_cones > 0 {
            self.loss(
                StepLossCode::EllipticalConeReduced,
                format!(
                    "{elliptical_cones} elliptical cone surface(s) were reduced to circular STEP CONICAL_SURFACE carriers"
                ),
            );
        }
        if !self.curveless_edges.is_empty() {
            self.loss(
                StepLossCode::CurvelessEdgeOmitted,
                format!(
                    "{} edge(s) have no typed 3D curve or carry a STEP-unsupported transform and were omitted from \
                     their edge loops (STEP EDGE_CURVE requires a 3D curve)",
                    self.curveless_edges.len()
                ),
            );
        }
        if !self.unknown_surface_faces.is_empty() {
            self.loss(
                StepLossCode::UnknownSurfaceFaceOmitted,
                format!(
                    "{} face(s) rest on an unknown or STEP-unsupported surface and were omitted \
                     from the STEP shell (an ADVANCED_FACE requires a surface); their \
                     topology remains in the IR",
                    self.unknown_surface_faces.len()
                ),
            );
        }
        self.unwritten_geometry_carriers.retain(|id| {
            !self.surface_refs.contains_key(id.as_str())
                && !self.curve_refs.contains_key(id.as_str())
                && !self.point_refs.contains_key(id.as_str())
        });
        if !self.unwritten_geometry_carriers.is_empty() {
            let carriers = self
                .unwritten_geometry_carriers
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::GeometryCarrierNotWritten,
                format!(
                    "{} geometry carrier(s) were not written: {carriers}",
                    self.unwritten_geometry_carriers.len()
                ),
            );
        }
        if !self.unwritten_pcurve_carriers.is_empty() {
            let pcurves = self
                .unwritten_pcurve_carriers
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::PcurveCarrierUnwritable,
                format!(
                    "{} coedge pcurve carrier(s) use geometry or surface references that were not writable: {pcurves}",
                    self.unwritten_pcurve_carriers.len()
                ),
            );
        }
        if !self.missing_parent_products.is_empty() {
            let occurrences = self
                .missing_parent_products
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AssemblyOccurrenceOmittedNoParentProduct,
                format!(
                    "{} assembly occurrence(s) were omitted because their parent has no local product definition: {occurrences}",
                    self.missing_parent_products.len()
                ),
            );
        }
        if !self.empty_regions.is_empty() {
            let regions = self
                .empty_regions
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::RegionNoShellList,
                format!(
                    "{} region(s) have no shell list and were not written to STEP: {regions}",
                    self.empty_regions.len()
                ),
            );
        }
        if !self.empty_wire_regions.is_empty() {
            let regions = self
                .empty_wire_regions
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::WireRegionNoConnectedEdgeSet,
                format!(
                    "{} wire region(s) had no writable connected edge set and were not written to STEP: {regions}",
                    self.empty_wire_regions.len()
                ),
            );
        }
        if !self.missing_wire_shells.is_empty() {
            let shells = self
                .missing_wire_shells
                .iter()
                .map(|(region, shell)| format!("'{region}' -> '{shell}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::WireRegionMissingShell,
                format!(
                    "{} wire region/shell relation(s) referenced missing shell records: {shells}",
                    self.missing_wire_shells.len()
                ),
            );
        }
        if !self.hidden_bodies_without_items.is_empty() {
            let bodies = self
                .hidden_bodies_without_items
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::HiddenBodyOmitted,
                format!(
                    "{} hidden body/bodies had no emitted STEP item and were omitted from INVISIBILITY: {bodies}",
                    self.hidden_bodies_without_items.len()
                ),
            );
        }
        if !self.hidden_presentation_layers_without_items.is_empty() {
            let layers = self
                .hidden_presentation_layers_without_items
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::HiddenPresentationLayerOmitted,
                format!(
                    "{} hidden presentation layer(s) had no emitted STEP assignment and were omitted from INVISIBILITY: {layers}",
                    self.hidden_presentation_layers_without_items.len()
                ),
            );
        }
        if !self.dangling_appearance_bindings.is_empty() {
            let bindings = self
                .dangling_appearance_bindings
                .iter()
                .map(|(binding, appearance)| format!("'{binding}' -> '{appearance}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AppearanceBindingMissingAsset,
                format!(
                    "{} appearance binding(s) reference missing appearance assets and were not written: {bindings}",
                    self.dangling_appearance_bindings.len()
                ),
            );
        }
        if !self.colorless_appearance_bindings.is_empty() {
            let bindings = self
                .colorless_appearance_bindings
                .iter()
                .map(|(binding, appearance)| format!("'{binding}' -> '{appearance}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AppearanceBindingNoBaseColor,
                format!(
                    "{} appearance binding(s) reference appearances without a base color and were not written: {bindings}",
                    self.colorless_appearance_bindings.len()
                ),
            );
        }
        let missing_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter(|use_| !self.pcurves.contains_key(use_.pcurve.as_str()))
            .count();
        if missing_pcurve_count > 0 {
            self.loss(
                StepLossCode::CoedgePcurveNoGeometry,
                format!(
                    "{missing_pcurve_count} coedge pcurve reference(s) have no geometry and were not written"
                ),
            );
        }
        let reduced_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter_map(|use_| self.pcurves.get(use_.pcurve.as_str()))
            .filter(|pcurve| {
                pcurve.wrapper_reversed().is_some()
                    || pcurve.native_tail_flags().is_some()
                    || pcurve.parameter_range().is_some()
                    || pcurve.fit_tolerance().is_some()
            })
            .count();
        if reduced_pcurve_count > 0 {
            self.loss(
                StepLossCode::CoedgePcurveNativeMetadata,
                format!(
                    "{reduced_pcurve_count} emitted coedge pcurve(s) carry native-only metadata not represented in STEP"
                ),
            );
        }
        let pcurve_use_metadata_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .chain(self.ir.model.loops.iter().flat_map(Loop::vertex_pcurves))
            .filter(|use_| use_.isoparametric.is_some() || use_.parameter_range.is_some())
            .count();
        if pcurve_use_metadata_count > 0 {
            self.loss(
                StepLossCode::PcurveUseNativeMetadata,
                format!(
                    "{pcurve_use_metadata_count} pcurve use(s) carry native-only parameter metadata not represented in STEP"
                ),
            );
        }
        let coedge_use_curve_metadata_count = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count();
        if coedge_use_curve_metadata_count > 0 {
            self.loss(
                StepLossCode::CoedgeUseCurveNotRepresented,
                format!(
                    "{coedge_use_curve_metadata_count} coedge-local 3D curve use(s) were not represented in STEP"
                ),
            );
        }
        // EDGE_CURVE carries its bounded domain through the two vertex
        // endpoints. The IR parameter interval is a derived parameterization
        // detail, not a separate STEP attribute, so it is not a loss here.
        let topology_metadata = [
            (
                "face tolerance",
                self.ir
                    .model
                    .faces
                    .iter()
                    .filter(|face| face.tolerance.is_some())
                    .count(),
            ),
            (
                "edge tolerance",
                self.ir
                    .model
                    .edges
                    .iter()
                    .filter(|edge| edge.tolerance.is_some())
                    .count(),
            ),
            (
                "vertex tolerance",
                self.ir
                    .model
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.tolerance.is_some())
                    .count(),
            ),
        ];
        let topology_metadata_count = topology_metadata
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>();
        if topology_metadata_count > 0 {
            let details = topology_metadata
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::TopologyMetadataNotRepresented,
                format!(
                    "{topology_metadata_count} topology metadata value(s) were not represented in STEP: {details}"
                ),
            );
        }
        if !self.ir.model.subds.is_empty() {
            self.loss(
                StepLossCode::SubdOmitted,
                format!(
                    "{} subdivision surface(s) were omitted because this STEP writer \
                     does not encode SubD control cages",
                    self.ir.model.subds.len()
                ),
            );
        }
        let design_arenas = [
            ("feature", self.ir.model.features.len()),
            (
                "feature input topology",
                self.ir.model.feature_input_topologies.len(),
            ),
            (
                "feature result topology",
                self.ir.model.feature_result_topologies.len(),
            ),
            ("configuration", self.ir.model.configurations.len()),
            ("parameter", self.ir.model.parameters.len()),
            ("sketch", self.ir.model.sketches.len()),
            ("sketch entity", self.ir.model.sketch_entities.len()),
            ("sketch constraint", self.ir.model.sketch_constraints.len()),
            ("spatial sketch", self.ir.model.spatial_sketches.len()),
            (
                "spatial sketch entity",
                self.ir.model.spatial_sketch_entities.len(),
            ),
            (
                "spatial sketch constraint",
                self.ir.model.spatial_sketch_constraints.len(),
            ),
            ("spreadsheet", self.ir.model.spreadsheets.len()),
        ];
        let design_record_count = design_arenas.iter().map(|(_, count)| count).sum::<usize>();
        if design_record_count > 0 {
            let details = design_arenas
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::ParametricDesignRecordsOmitted,
                format!(
                    "{design_record_count} parametric/design record(s) were not represented in STEP: {details}"
                ),
            );
        }
        let presentation_arenas = [
            ("drawing", self.ir.model.drawings.len()),
            (
                "presentation document",
                self.ir.model.presentation_documents.len(),
            ),
            ("view presentation", self.ir.model.view_presentations.len()),
        ];
        let presentation_record_count = presentation_arenas
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>();
        if presentation_record_count > 0 {
            let details = presentation_arenas
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::DrawingPresentationRecordsOmitted,
                format!(
                    "{presentation_record_count} drawing/presentation record(s) were not represented in STEP: {details}"
                ),
            );
        }
        if !self.ir.model.semantic_annotations.is_empty() {
            self.loss(
                StepLossCode::SemanticAnnotationOmitted,
                format!(
                    "{} semantic annotation(s) were not represented in STEP",
                    self.ir.model.semantic_annotations.len()
                ),
            );
        }
        if !self.ir.model.assets.is_empty() {
            self.loss(
                StepLossCode::DocumentAssetOmitted,
                format!(
                    "{} document asset(s) were not represented in STEP",
                    self.ir.model.assets.len()
                ),
            );
        }
        if !self.ir.model.assembly_joints.is_empty() {
            self.loss(
                StepLossCode::AssemblyJointOmitted,
                format!(
                    "{} assembly joint(s) were not represented in STEP",
                    self.ir.model.assembly_joints.len()
                ),
            );
        }
        let non_part_products = self
            .ir
            .model
            .product_definitions
            .iter()
            .filter(|product| {
                !matches!(
                    &product.kind,
                    cadmpeg_ir::products::ProductDefinitionKind::Part
                )
            })
            .count();
        if non_part_products > 0 {
            self.loss(
                StepLossCode::ProductNonPartKind,
                format!(
                    "{non_part_products} product definition(s) use a non-part kind not represented in STEP"
                ),
            );
        }
        let bom_property_count = self
            .ir
            .model
            .product_definitions
            .iter()
            .map(|product| product.bom_properties.len())
            .sum::<usize>();
        if bom_property_count > 0 {
            self.loss(
                StepLossCode::ProductBomPropertyOmitted,
                format!(
                    "{bom_property_count} product BOM property value(s) were not represented in STEP"
                ),
            );
        }
        let external_occurrences = self
            .ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    &occurrence.prototype,
                    cadmpeg_ir::products::PrototypeReference::External { .. }
                )
            })
            .count();
        if external_occurrences > 0 {
            self.loss(
                StepLossCode::OccurrenceExternalProduct,
                format!(
                    "{external_occurrences} occurrence(s) reference external product definitions and were not represented in STEP"
                ),
            );
        }
        let unresolved_occurrences = self
            .ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    &occurrence.prototype,
                    cadmpeg_ir::products::PrototypeReference::Unresolved {}
                )
            })
            .count();
        if unresolved_occurrences > 0 {
            self.loss(
                StepLossCode::OccurrenceNoWritableProduct,
                format!(
                    "{unresolved_occurrences} occurrence(s) have no writable product definition"
                ),
            );
        }
        let occurrence_metadata = self
            .ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| {
                let link_metadata = occurrence.link.as_ref().map_or(0, |link| {
                    usize::from(!link.linked_subelements().is_empty())
                        + usize::from(link.element_component().is_some())
                        + usize::from(link.claim_child().is_some())
                        + link.copy_on_change().map_or(0, |copy| {
                            1 + usize::from(copy.source.is_some())
                                + usize::from(copy.group.is_some())
                                + usize::from(copy.touched.is_some())
                        })
                });
                usize::from(occurrence.visible.is_some()) + link_metadata
            })
            .sum::<usize>();
        if occurrence_metadata > 0 {
            self.loss(
                StepLossCode::OccurrenceMetadataOmitted,
                format!(
                    "{occurrence_metadata} occurrence metadata value(s) were not represented in STEP"
                ),
            );
        }
        if let Some(unwritten_pmi) = self.ir.model.pmi.len().checked_sub(self.written_pmi) {
            if unwritten_pmi > 0 {
                // Naming the target that would carry these is only honest when the
                // schema gate is why they were dropped. A target that supports
                // semantic PMI and still left annotations unwritten dropped them for
                // some other reason, and pointing at another target would misdirect.
                let message = if self.schema.supports_semantic_pmi() {
                    format!("{unwritten_pmi} PMI annotation(s) were not written to STEP")
                } else {
                    format!(
                        "{unwritten_pmi} PMI annotation(s) were not written to STEP; {} does not carry semantic PMI, which requires an AP242 edition target",
                        self.schema.file_schema()
                    )
                };
                self.loss(StepLossCode::PmiAnnotationNotWritten, message);
            }
        }
        // STEP-native source associations identify records already represented
        // by the writer's own STEP graph. They are not lossy foreign-source
        // metadata. Keep strict-mode rejection for associations from other
        // codecs, which this writer cannot reproduce.
        let source_object_count = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                surface
                    .source_object
                    .as_ref()
                    .is_some_and(|source| source.format != cadmpeg_ir::CodecFormat::Step)
            })
            .count()
            + self
                .ir
                .model
                .curves
                .iter()
                .filter(|curve| {
                    curve
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != cadmpeg_ir::CodecFormat::Step)
                })
                .count()
            + self
                .ir
                .model
                .subds
                .iter()
                .filter(|subd| {
                    subd.source_object
                        .as_ref()
                        .is_some_and(|source| source.format != cadmpeg_ir::CodecFormat::Step)
                })
                .count()
            + self
                .ir
                .model
                .tessellations
                .iter()
                .filter(|tessellation| {
                    tessellation
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != cadmpeg_ir::CodecFormat::Step)
                })
                .count();
        let source_object_count = source_object_count
            + self
                .ir
                .model
                .points
                .iter()
                .filter(|point| {
                    point
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != cadmpeg_ir::CodecFormat::Step)
                })
                .count();
        if source_object_count > 0 {
            self.loss(
                StepLossCode::SourceAssociationOmitted,
                format!(
                    "{source_object_count} source-object association(s) were not represented in STEP"
                ),
            );
        }
        let unknown_count = self
            .ir
            .native
            .loss_counts()
            .into_iter()
            .filter(|count| count.kind == "unknowns")
            .map(|count| count.count.get())
            .sum::<usize>();
        if unknown_count > 0 {
            self.loss(
                StepLossCode::PassthroughRecordOmitted,
                format!("{unknown_count} uninterpreted passthrough record(s) were not represented in STEP"),
            );
        }
        if self.unstyled_colors > 0 {
            self.loss(
                StepLossCode::DisplayColorUnstyled,
                format!(
                    "{} display color(s) had no emitted STEP item and were not written \
                     to STEP presentation",
                    self.unstyled_colors
                ),
            );
        }
        let appearance_binding_target_conflicts = self.appearance_binding_target_conflicts.clone();
        for ((target_kind, target_id), bindings) in appearance_binding_target_conflicts {
            let binding_ids = bindings
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AppearanceBindingTargetConflict,
                format!(
                    "appearance bindings {binding_ids} have conflicting target style projections for {target_kind} '{target_id}' and were not written"
                ),
            );
        }
        let lossy_appearances = self
            .ir
            .model
            .appearances
            .iter()
            .filter(|appearance| {
                let bindings = self
                    .ir
                    .model
                    .appearance_bindings
                    .iter()
                    .filter(|binding| binding.appearance == appearance.id)
                    .collect::<Vec<_>>();
                let alpha_unwritable = appearance.base_color.is_some_and(|color| {
                    color.a() != 1.0
                        && bindings.iter().any(|binding| match &binding.target {
                            AppearanceTarget::Curve(_)
                            | AppearanceTarget::Edge(_)
                            | AppearanceTarget::Point(_)
                            | AppearanceTarget::Vertex(_) => true,
                            AppearanceTarget::Body(body) => self
                                .bodies
                                .get(body.as_str())
                                .is_some_and(|body| body.kind == BodyKind::Wire),
                            _ => false,
                        })
                });
                appearance.asset_guid.is_some()
                    || appearance.visual_guid.is_some()
                    || appearance.physical_token.is_some()
                    || appearance
                        .schema
                        .as_deref()
                        .is_some_and(|schema| schema != "step_surface_style")
                    || appearance.category.is_some()
                    || !appearance.properties.is_empty()
                    || alpha_unwritable
                    || bindings.is_empty()
                    || bindings.iter().any(|binding| {
                        !self.written_appearance_bindings.contains(&binding.id)
                            && !self.conflicted_appearance_bindings.contains(&binding.id)
                    })
            })
            .count();
        if lossy_appearances > 0 {
            self.loss(
                StepLossCode::AppearanceReducedToBaseColor,
                format!(
                    "{lossy_appearances} appearance asset(s) were reduced to STYLED_ITEM base colors; \
                     unsupported schemas, textures, and shader properties were not written to STEP"
                ),
            );
        }
        let lossy_binding_metadata = self
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.object_type.is_some() || !binding.channels.is_empty())
            .count();
        if lossy_binding_metadata > 0 {
            self.loss(
                StepLossCode::AppearanceBindingMetadataReduced,
                format!(
                    "{lossy_binding_metadata} appearance binding(s) carry source object or channel metadata not represented in STEP"
                ),
            );
        }
        if !self.ir.model.attributes.is_empty() {
            self.loss(
                StepLossCode::SourceAttributeNotWritten,
                format!(
                    "{} source attribute record(s) were not written to STEP",
                    self.ir.model.attributes.len()
                ),
            );
        }
        let reduced_surfaces: BTreeSet<_> = self
            .procedural_surfaces
            .iter()
            .filter(|(owner, procedural)| {
                self.surface_refs.contains_key::<str>(owner)
                    && !self
                        .written_procedural_surfaces
                        .contains(procedural.id.as_str())
            })
            .map(|(_, procedural)| procedural.id.as_str())
            .collect();
        let reduced_curves: BTreeSet<_> = self
            .procedural_curves
            .iter()
            .filter(|(owner, procedural)| {
                self.curve_refs.contains_key::<str>(owner)
                    && !self
                        .written_procedural_curves
                        .contains(procedural.id.as_str())
            })
            .map(|(_, procedural)| procedural.id.as_str())
            .collect();
        let omitted_surface_count = self
            .ir
            .model
            .procedural_surfaces
            .iter()
            .filter(|procedural| {
                !self
                    .written_procedural_surfaces
                    .contains(procedural.id.as_str())
                    && !reduced_surfaces.contains(procedural.id.as_str())
            })
            .count();
        let omitted_curve_count = self
            .ir
            .model
            .procedural_curves
            .iter()
            .filter(|procedural| {
                !self
                    .written_procedural_curves
                    .contains(procedural.id.as_str())
                    && !reduced_curves.contains(procedural.id.as_str())
            })
            .count();
        let procedural_surface_count = reduced_surfaces.len();
        let procedural_curve_count = reduced_curves.len();
        if procedural_surface_count > 0 || procedural_curve_count > 0 {
            self.loss(
                StepLossCode::ProceduralReducedToCarrier,
                format!(
                    "{procedural_surface_count} procedural surface definition(s) and {procedural_curve_count} procedural curve definition(s) were reduced to their solved STEP carriers"
                ),
            );
        }
        if omitted_surface_count > 0 || omitted_curve_count > 0 {
            self.loss(
                StepLossCode::ProceduralDefinitionNotWritten,
                format!("{omitted_surface_count} procedural surface definition(s) and {omitted_curve_count} procedural curve definition(s) had no emitted STEP construction or solved carrier"),
            );
        }
        let source_native_records: usize = self
            .ir
            .native
            .loss_counts()
            .iter()
            .filter(|loss| loss.kind != "unknowns")
            .map(|loss| loss.count.get())
            .sum();
        if source_native_records > 0 {
            self.loss(
                StepLossCode::SourceNativeRecordOmitted,
                format!(
                    "{source_native_records} source-native record(s) were not represented in STEP"
                ),
            );
        }
    }

    fn finish_outcome(&self) -> StepWriteOutcome {
        StepWriteOutcome {
            census: cadmpeg_ir::report::export::EntityCensus {
                basis: cadmpeg_ir::report::export::CensusBasis::TargetRecords,
                counts: cadmpeg_ir::CensusKey::count_map(self.emitter.counts()),
            },
            losses: self.losses.clone(),
            notes: Vec::new(),
        }
    }
}

fn datum_target_form_text(form: &DatumTargetForm) -> &str {
    match form {
        DatumTargetForm::Point => "point",
        DatumTargetForm::Line => "line",
        DatumTargetForm::Rectangle => "rectangle",
        DatumTargetForm::Circle => "circle",
        DatumTargetForm::CircularCurve => "circular curve",
        DatumTargetForm::Other(value) => value,
    }
}

fn is_identity(rows: &[[f64; 4]; 4]) -> bool {
    for (i, row) in rows.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            let expect = if i == j { 1.0 } else { 0.0 };
            if (v - expect).abs() > EPS_IDENTITY {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod accounting_tests;
#[cfg(test)]
mod construction_tests;
