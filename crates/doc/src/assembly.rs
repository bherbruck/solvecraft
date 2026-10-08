//! Components and occurrences: the design as a tree.
//!
//! The root component (id 0) is the design. Every other component lives in a parent component
//! through one or more occurrences, each with a rigid transform (instances share the component's
//! contents; a "Paste New" copy is a separate component). A component's sketches, construction
//! geometry and features are authored in its own frame (its origin); the model is evaluated in
//! those frames and placed in the world through the occurrence transforms when bodies are
//! gathered for display, measuring and export (`world_bodies`). Commands take world coordinates
//! and the engine maps a new feature's geometry into the active component's frame.

use serde::{Deserialize, Serialize};
use solvecraft_geom::{Plane, Vec3};

use crate::{AxisRef, DocError, Document, FeatureKind, PatternKind, PlaneRef, Result};

/// Column-major 4×4 affine transform (`m[col][row]`).
pub type Mat = [[f64; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

pub fn mat_mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0.0; 4]; 4];
    for (c, col) in r.iter_mut().enumerate() {
        for (row, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

pub fn apply_point(m: &Mat, p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

pub fn apply_vector(m: &Mat, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
        m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
        m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
    )
}

pub fn is_identity(m: &Mat) -> bool {
    m.iter().flatten().zip(IDENTITY.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12)
}

/// Inverse of an affine transform.
pub fn mat_inverse(m: &Mat) -> Option<Mat> {
    let a = |r: usize, c: usize| m[c][r];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-14 || !det.is_finite() {
        return None;
    }
    let inv = |r: usize, c: usize| -> f64 {
        // Cofactor of (c, r) / det.
        let (r0, r1) = match c {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let (c0, c1) = match r {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let minor = a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
        let sign = if (r + c).is_multiple_of(2) { 1.0 } else { -1.0 };
        sign * minor / det
    };
    let mut out = IDENTITY;
    for (c, col) in out.iter_mut().enumerate().take(3) {
        for (r, cell) in col.iter_mut().enumerate().take(3) {
            *cell = inv(r, c);
        }
    }
    let t = Vec3::new(m[3][0], m[3][1], m[3][2]);
    let it = apply_vector(&out, t);
    out[3] = [-it.x, -it.y, -it.z, 1.0];
    Some(out)
}

/// A rigid transform: rotation by `angle` about `axis` through `origin`, then a translation.
pub fn rigid(translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Mat {
    let k = axis.normalized().unwrap_or(Vec3::Z);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    let r = [
        [t * k.x * k.x + c, t * k.x * k.y + s * k.z, t * k.x * k.z - s * k.y],
        [t * k.x * k.y - s * k.z, t * k.y * k.y + c, t * k.y * k.z + s * k.x],
        [t * k.x * k.z + s * k.y, t * k.y * k.z - s * k.x, t * k.z * k.z + c],
    ];
    let mut m = IDENTITY;
    for (mc, rc) in m.iter_mut().zip(r) {
        mc[..3].copy_from_slice(&rc);
    }
    let ro = apply_vector(&m, origin);
    let tr = origin - ro + translate;
    m[3] = [tr.x, tr.y, tr.z, 1.0];
    m
}

/// A placement of a component inside a parent component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: u64,
    pub component: u64,
    /// The component it sits in (0 = the root).
    #[serde(default)]
    pub parent: u64,
    pub name: String,
    #[serde(default = "identity")]
    pub transform: Mat,
    /// Grounded occurrences don't move.
    #[serde(default)]
    pub grounded: bool,
}

fn identity() -> Mat {
    IDENTITY
}

fn map_plane(m: &Mat, p: &Plane) -> Plane {
    let (o, x, y) = (apply_point(m, p.origin), apply_vector(m, p.x), apply_vector(m, p.y));
    Plane::new(o, x, y).unwrap_or(*p)
}

fn map_plane_ref(m: &Mat, p: &mut PlaneRef) {
    match p {
        PlaneRef::Custom { plane } => *plane = map_plane(m, plane),
        PlaneRef::Offset { base, .. } => map_plane_ref(m, base),
        PlaneRef::AtAngle { base, axis_origin, axis_dir, .. } => {
            map_plane_ref(m, base);
            *axis_origin = apply_point(m, *axis_origin);
            *axis_dir = apply_vector(m, *axis_dir);
        }
        // Named origin planes and construction planes are the component's own.
        PlaneRef::Origin { .. } | PlaneRef::Construction { .. } => {}
    }
}

impl FeatureKind {
    /// Map the feature's world geometry (points, directions, explicit planes) by `m`.
    pub fn transform_geometry(&mut self, m: &Mat) {
        let pt = |p: &mut Vec3| *p = apply_point(m, *p);
        let dir = |v: &mut Vec3| *v = apply_vector(m, *v);
        match self {
            FeatureKind::Sketch { plane, .. }
            | FeatureKind::ConstructionPlane { plane }
            | FeatureKind::Split { plane, .. }
            | FeatureKind::Mirror { plane, .. } => map_plane_ref(m, plane),
            FeatureKind::Revolve { axis: AxisRef::Line { origin, dir: d }, .. } => {
                pt(origin);
                dir(d);
            }
            FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => edges.iter_mut().for_each(pt),
            FeatureKind::Shell { faces, .. } | FeatureKind::OffsetFace { faces, .. } => faces.iter_mut().for_each(pt),
            FeatureKind::Draft { faces, neutral, pull, .. } => {
                faces.iter_mut().for_each(pt);
                map_plane_ref(m, neutral);
                dir(pull);
            }
            FeatureKind::Box { corner, .. } => pt(corner),
            FeatureKind::Cylinder { base, axis, .. } => {
                pt(base);
                dir(axis);
            }
            FeatureKind::Sphere { center, .. } | FeatureKind::Torus { center, .. } => pt(center),
            FeatureKind::Pattern { pattern: PatternKind::Rectangular { dir1, dir2, .. }, .. } => {
                dir(dir1);
                if let Some(d) = dir2 {
                    dir(d);
                }
            }
            FeatureKind::Pattern { pattern: PatternKind::Circular { origin, axis, .. }, .. } => {
                pt(origin);
                dir(axis);
            }
            // Follows its sketch.
            FeatureKind::Pattern { pattern: PatternKind::Path { .. }, .. } => {}
            FeatureKind::Hole { position, direction, .. } => {
                pt(position);
                dir(direction);
            }
            FeatureKind::Thread { face, .. } => pt(face),
            FeatureKind::Coil { base, axis, .. } => {
                pt(base);
                dir(axis);
            }
            FeatureKind::ReplaceFace { faces, target, .. } => {
                faces.iter_mut().for_each(pt);
                map_plane_ref(m, target);
            }
            FeatureKind::Align { from, to, from_normal, to_normal, .. } => {
                pt(from);
                pt(to);
                if let Some(n) = from_normal {
                    dir(n);
                }
                if let Some(n) = to_normal {
                    dir(n);
                }
            }
            FeatureKind::Scale { origin, .. } => pt(origin),
            FeatureKind::Move { rotate_axis, .. } => {
                if let Some(a) = rotate_axis {
                    dir(a);
                }
            }
            FeatureKind::Revolve { .. }
            | FeatureKind::Extrude { .. }
            | FeatureKind::Combine { .. }
            | FeatureKind::Loft { .. }
            | FeatureKind::Sweep { .. }
            | FeatureKind::BoundingSolid { .. }
            | FeatureKind::Pipe { .. }
            | FeatureKind::Emboss { .. }
            | FeatureKind::Rib { .. }
            | FeatureKind::Remove { .. }
            | FeatureKind::Import { .. }
            | FeatureKind::MeshImport { .. } => {}
        }
    }
}

impl Document {
    /// The first occurrence of a component (its placement for authoring).
    pub fn occurrence_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.occurrences.iter().find(|o| o.component == component)
    }

    /// World transform of a component's frame (through its first occurrence and its parents').
    pub fn component_transform(&self, component: u64) -> Mat {
        let mut m = IDENTITY;
        let mut c = component;
        for _ in 0..1000 {
            if c == 0 {
                break;
            }
            let Some(o) = self.occurrence_of(c) else { break };
            m = mat_mul(&o.transform, &m);
            c = o.parent;
        }
        m
    }

    /// Every placement of each component: (component, occurrence path ids, world transform).
    pub fn placements(&self) -> Vec<(u64, Vec<u64>, Mat)> {
        let mut out = vec![(0, Vec::new(), IDENTITY)];
        let mut stack: Vec<(u64, Vec<u64>, Mat)> = vec![(0, Vec::new(), IDENTITY)];
        while let Some((comp, path, m)) = stack.pop() {
            if path.len() > 64 || out.len() > 100_000 {
                break;
            }
            for o in self.occurrences.iter().filter(|o| o.parent == comp) {
                let mut p = path.clone();
                p.push(o.id);
                let w = mat_mul(&m, &o.transform);
                out.push((o.component, p.clone(), w));
                stack.push((o.component, p, w));
            }
        }
        out
    }

    /// Is `a` inside `b` (or `b` itself)?
    pub fn component_within(&self, a: u64, b: u64) -> bool {
        let mut c = a;
        for _ in 0..1000 {
            if c == b {
                return true;
            }
            if c == 0 {
                return false;
            }
            match self.components.iter().find(|x| x.id == c) {
                Some(x) => c = x.parent,
                None => return false,
            }
        }
        false
    }

    /// Add an occurrence of an existing component into `parent`.
    pub fn add_occurrence(&mut self, component: u64, parent: u64, transform: Mat) -> Result<u64> {
        if component == 0 || !self.components.iter().any(|c| c.id == component) {
            return Err(DocError::Unknown(format!("component {component}")));
        }
        if self.component_within(parent, component) {
            return Err(DocError::Invalid("a component can't contain itself".into()));
        }
        if self.occurrences.len() >= 100_000 {
            return Err(DocError::Invalid("too many occurrences".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let cname = self.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
        let k = self.occurrences.iter().filter(|o| o.component == component).count() + 1;
        self.occurrences.push(crate::Occurrence { id, component, parent, name: format!("{cname}:{k}"), transform, grounded: false });
        Ok(id)
    }
}
