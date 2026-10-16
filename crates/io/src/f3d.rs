//! Autodesk Fusion `.f3d` / `.f3z` import via [cadmpeg](https://github.com/cadmpeg/cadmpeg)
//! (B-rep decode → STEP → existing STEP import, with mesh fallback when B-rep does not transfer).

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_codec_step::StepCodec;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{Codec, DecodeOptions};
use solidcraft_geom::Vec3;
use std::collections::{HashMap, HashSet};

use crate::threemf::MeshObject;
use crate::{IoError, Result};

fn f3d_decode_options() -> DecodeOptions {
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_collection_items = 128_000_000;
    // Large offline Fusion caches retain many compressed streams while decoding.
    policy.limits.max_retained_bytes = 4 * 1024 * 1024 * 1024;
    policy.limits.max_materialized_bytes = 4 * 1024 * 1024 * 1024;
    policy.limits.max_work_units = 8_000_000_000;
    DecodeOptions { container_only: false, policy }
}

/// Largest Fusion archive we read (offline caches can exceed 200 MB).
pub const MAX_F3D_BYTES: usize = 512 << 20;

/// Fusion ZIP without a SolidCraft `container.json` (name-independent sniff).
pub fn looks_like_fusion_archive(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"PK\x03\x04") {
        return false;
    }
    let Ok(zip) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    // Fusion stores `Manifest.dat` with a non-deflate method; `by_name` then fails even though
    // the entry is listed. Sniff from central-directory names only.
    let mut manifest = false;
    let mut design = false;
    for name in zip.file_names() {
        if name == "Manifest.dat" || name.ends_with("/Manifest.dat") {
            manifest = true;
        }
        if name.contains("FusionDesignSegmentType") || name.contains("Breps.BlobParts") {
            design = true;
        }
        if manifest && design {
            return true;
        }
    }
    manifest && design
}

/// What opening a Fusion file produced.
pub enum F3dOpen {
    /// Solid B-rep exported to STEP for the Import feature.
    Brep { step: Vec<u8>, warnings: Vec<String> },
    /// Cached/display tessellation from the Fusion file (mesh bodies).
    Mesh { objects: Vec<MeshObject>, warnings: Vec<String> },
}

fn transform_is_identity(t: Transform) -> bool {
    let rows = t.rows();
    const EPS: f64 = 1e-9;
    (rows[0][0] - 1.0).abs() < EPS
        && rows[0][1].abs() < EPS
        && rows[0][2].abs() < EPS
        && rows[0][3].abs() < EPS
        && rows[1][0].abs() < EPS
        && (rows[1][1] - 1.0).abs() < EPS
        && rows[1][2].abs() < EPS
        && rows[1][3].abs() < EPS
        && rows[2][0].abs() < EPS
        && rows[2][1].abs() < EPS
        && (rows[2][2] - 1.0).abs() < EPS
        && rows[2][3].abs() < EPS
}

fn occurrence_world_transform(
    occ: &Occurrence,
    by_id: &HashMap<&str, &Occurrence>,
    memo: &mut HashMap<String, Transform>,
    active: &mut HashSet<String>,
) -> Option<Transform> {
    if let Some(t) = memo.get(occ.id.as_str()) {
        return Some(*t);
    }
    if !active.insert(occ.id.as_str().to_string()) {
        return None;
    }
    let parent = match &occ.parent {
        OccurrenceParent::Root {} => Transform::identity(),
        OccurrenceParent::Occurrence { occurrence } => {
            let p = by_id.get(occurrence.as_str())?;
            occurrence_world_transform(p, by_id, memo, active)?
        }
    };
    let local = occ.effective_transform().ok()?;
    let world = parent.compose(local).ok()?;
    memo.insert(occ.id.as_str().to_string(), world);
    active.remove(occ.id.as_str());
    Some(world)
}

/// When Fusion stores each component's B-rep at the origin but places instances via the
/// occurrence tree, STEP export needs per-body transforms (or a full product graph). Baking
/// world occurrence transforms onto bodies fixes flat assemblies that would otherwise stack.
fn bake_component_placements_into_bodies(model: &mut Model) {
    if model.product_definitions.is_empty() || model.occurrences.is_empty() {
        return;
    }
    if !model.bodies.iter().all(|b| b.transform.is_none_or(transform_is_identity)) {
        return;
    }
    let by_id: HashMap<&str, &Occurrence> = model.occurrences.iter().map(|o| (o.id.as_str(), o)).collect();
    let mut memo = HashMap::new();
    let mut active = HashSet::new();
    let mut placement_by_product: HashMap<ProductDefinitionId, Transform> = HashMap::new();
    for occ in &model.occurrences {
        let PrototypeReference::Local { definition } = &occ.prototype else {
            continue;
        };
        let Some(world) = occurrence_world_transform(occ, &by_id, &mut memo, &mut active) else {
            continue;
        };
        match placement_by_product.get(definition) {
            None => {
                placement_by_product.insert(definition.clone(), world);
            }
            Some(existing) if existing.rows() == world.rows() => {}
            Some(_) => {
                // Multiple instances of the same definition at different transforms: leave
                // assembly export to carry placements.
                placement_by_product.remove(definition);
            }
        }
    }
    if placement_by_product.is_empty() {
        return;
    }
    let mut body_world: HashMap<String, Transform> = HashMap::new();
    for product in &model.product_definitions {
        let Some(world) = placement_by_product.get(&product.id) else {
            continue;
        };
        for body_id in &product.bodies {
            body_world.insert(body_id.as_str().to_string(), *world);
        }
    }
    for body in &mut model.bodies {
        if body.transform.is_some_and(|t| !transform_is_identity(t)) {
            continue;
        }
        if let Some(world) = body_world.get(body.id.as_str())
            && !transform_is_identity(*world)
        {
            body.transform = Some(*world);
        }
    }
}

fn mesh_objects_or_none(ir: &cadmpeg_ir::document::CadIr, stem: &str) -> Option<Vec<MeshObject>> {
    let objects = mesh_objects_from_ir(ir, stem);
    (!objects.is_empty()).then_some(objects)
}

fn brep_step_from_ir(ir: &cadmpeg_ir::document::CadIr, warnings: &mut Vec<String>) -> std::result::Result<Vec<u8>, String> {
    let plan = match StepCodec::default().plan(EncodeInput::new(ir, None), TargetRequest::Inherit) {
        Ok(plan) => plan,
        Err(e) => return Err(e.to_string()),
    };
    for loss in &plan.report().losses {
        if matches!(loss.severity, Severity::Warning | Severity::Info) {
            warnings.push(loss.message.clone());
        }
    }
    let mut step = Vec::new();
    if let Err(e) = plan.write_to(&mut step) {
        return Err(e.to_string());
    }
    Ok(step)
}

fn mesh_objects_from_ir(ir: &cadmpeg_ir::document::CadIr, stem: &str) -> Vec<MeshObject> {
    let mut out = Vec::new();
    for (index, tess) in ir.model.tessellations.iter().enumerate() {
        let tris = tess.triangles();
        if tris.is_empty() {
            continue;
        }
        let positions: Vec<Vec3> = tess
            .vertices()
            .iter()
            .map(|p| {
                let q = p.get();
                Vec3::new(q.x, q.y, q.z)
            })
            .collect();
        if positions.len() < 3 {
            continue;
        }
        let name = tess.id.to_string();
        let name = if name.trim().is_empty() || name.starts_with('(') {
            if ir.model.tessellations.len() == 1 { stem.to_string() } else { format!("{stem} ({index})") }
        } else {
            name
        };
        out.push(MeshObject { name, positions, triangles: tris, ..Default::default() });
    }
    out
}

fn decode(bytes: &[u8], file: &str) -> Result<(cadmpeg_ir::DecodeResult, Vec<String>)> {
    if bytes.len() > MAX_F3D_BYTES {
        return Err(IoError::Invalid(format!("{file}: Fusion file too large ({} MB, limit {} MB)", bytes.len() >> 20, MAX_F3D_BYTES >> 20)));
    }
    if !looks_like_fusion_archive(bytes) {
        return Err(IoError::Invalid(format!("{file}: not a Fusion .f3d/.f3z archive")));
    }
    let mut input = Cursor::new(bytes);
    let decoded = F3dCodec.decode(&mut input, &f3d_decode_options()).map_err(|e| IoError::Invalid(format!("{file}: Fusion decode failed: {e}")))?;
    let report = decoded.report();
    let mut warnings: Vec<String> =
        report.losses.iter().filter(|l| matches!(l.severity, Severity::Warning | Severity::Info)).map(|l| l.message.clone()).collect();
    warnings.extend(report.notes.iter().cloned());
    Ok((decoded, warnings))
}

/// Open a Fusion archive: prefer exact B-rep; fall back to tessellation meshes when B-rep does not transfer.
pub fn open(bytes: &[u8], file: &str) -> Result<F3dOpen> {
    let stem = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Fusion".into());
    let (decoded, mut warnings) = decode(bytes, file)?;
    let report = decoded.report();
    if report.geometry_transferred() {
        let (mut ir, _, _) = decoded.into_parts();
        bake_component_placements_into_bodies(&mut ir.model);
        match brep_step_from_ir(&ir, &mut warnings) {
            Ok(step) => return Ok(F3dOpen::Brep { step, warnings }),
            Err(step_err) => {
                if let Some(objects) = mesh_objects_or_none(&ir, &stem) {
                    warnings.push(format!("Fusion B-rep did not export to STEP ({step_err}); opened cached mesh geometry from the file"));
                    return Ok(F3dOpen::Mesh { objects, warnings });
                }
                return Err(IoError::Invalid(format!("{file}: STEP export failed: {step_err}")));
            }
        }
    }
    let detail = report
        .losses
        .iter()
        .find(|l| l.severity == Severity::Blocking || l.severity == Severity::Error)
        .map(|l| l.message.clone())
        .unwrap_or_else(|| "no solid or mesh geometry transferred".into());
    let (ir, _, _) = decoded.into_parts();
    if let Some(objects) = mesh_objects_or_none(&ir, &stem) {
        warnings.push("Fusion B-rep did not transfer; opened cached mesh geometry from the file".into());
        return Ok(F3dOpen::Mesh { objects, warnings });
    }
    Err(IoError::Invalid(format!("{file}: {detail}")))
}
