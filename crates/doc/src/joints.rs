//! Joints between occurrences: joint origins (snap frames on component geometry, re-found on
//! every solve so they follow edits), joints (rigid, revolute, slider, cylindrical, pin-slot,
//! planar, ball) with their motion values and limits, motion links, and the solver that places
//! occurrences from the joint graph.
//!
//! A joint makes B's joint frame coincide with A's frame moved by the joint's motion:
//! `T_B · F_b = T_A · F_a · M(q) · Flip`, so `T_B = T_A · F_a · M(q) · Flip · F_b⁻¹`, where `T`
//! are occurrence world transforms, `F` the joint frames in each component's own frame, and `q`
//! the joint's values (angles in radians, distances in mm). The solver walks the graph from
//! grounded occurrences (and the root, which never moves); joints that close a loop are checked
//! and reported when they disagree.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;

use crate::{DocError, Document, IDENTITY, Mat, ModelState, Result, apply_point, apply_vector, mat_inverse, mat_mul};

const MAX_JOINTS: usize = 10_000;

/// Where a joint origin sits on a component's geometry (component coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Snap {
    /// An explicit frame (origin, z axis, x axis).
    Frame {
        origin: Vec3,
        z: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<Vec3>,
    },
    /// The centre of the planar face nearest `pick`, z along its outward normal.
    FaceCenter { pick: Vec3 },
    /// The centre of the circular end nearest `pick` of the cylindrical face there, z along its
    /// axis (pointing out of the face's end).
    CircleCenter { pick: Vec3 },
    /// A point, z along `z` (default +Z).
    Point {
        point: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<Vec3>,
    },
}

/// A joint origin: a snap on an occurrence's component (occurrence 0 = the root design).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointOrigin {
    pub occurrence: u64,
    pub snap: Snap,
}

/// A named joint origin (Joint Origin command), referenced by joints by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedOrigin {
    pub name: String,
    pub origin: JointOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    Rigid,
    Revolute,
    Slider,
    Cylindrical,
    PinSlot,
    Planar,
    Ball,
}

impl JointKind {
    pub fn parse(s: &str) -> Option<JointKind> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "rigid" => JointKind::Rigid,
            "revolute" | "rotate" => JointKind::Revolute,
            "slider" | "slide" => JointKind::Slider,
            "cylindrical" => JointKind::Cylindrical,
            "pinslot" => JointKind::PinSlot,
            "planar" => JointKind::Planar,
            "ball" => JointKind::Ball,
            _ => return None,
        })
    }
    /// Names of the joint's values, in order (angles in radians, distances in mm).
    pub fn dofs(self) -> &'static [&'static str] {
        match self {
            JointKind::Rigid => &[],
            JointKind::Revolute => &["angle"],
            JointKind::Slider => &["distance"],
            JointKind::Cylindrical => &["angle", "distance"],
            JointKind::PinSlot => &["angle", "slide"],
            JointKind::Planar => &["x", "y", "angle"],
            JointKind::Ball => &["yaw", "pitch", "roll"],
        }
    }
    pub fn is_angle(self, i: usize) -> bool {
        matches!(self.dofs().get(i), Some(&("angle" | "yaw" | "pitch" | "roll")))
    }
}

/// A joint between two occurrences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub id: u64,
    pub name: String,
    pub kind: JointKind,
    pub a: JointOrigin,
    pub b: JointOrigin,
    /// Current values of the joint's degrees of freedom.
    #[serde(default)]
    pub values: Vec<f64>,
    /// Per value: (min, max) limits.
    #[serde(default)]
    pub limits: Vec<Option<(f64, f64)>>,
    /// Turn B over (its z opposite to A's).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Distance from A's frame to B's along z, and a turn about it (radians).
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// Value of joint `b` (index `ib`) = ratio · value of joint `a` (index `ia`) + offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionLink {
    pub a: u64,
    #[serde(default)]
    pub ia: usize,
    pub b: u64,
    #[serde(default)]
    pub ib: usize,
    pub ratio: f64,
    #[serde(default)]
    pub offset: f64,
}

/// Joints, joint origins and motion links of a design.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joints: Vec<Joint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<NamedOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MotionLink>,
}

impl Assembly {
    pub fn is_empty(&self) -> bool {
        self.joints.is_empty() && self.origins.is_empty() && self.links.is_empty()
    }
}

/// A frame: origin and orthonormal axes, as a transform (local → parent).
fn frame(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    let z = z.normalized()?;
    let x = x.and_then(|x| (x - z * x.dot(z)).normalized()).unwrap_or_else(|| z.any_perp());
    let y = z.cross(x).normalized()?;
    Some([[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [o.x, o.y, o.z, 1.0]])
}

fn rot_z(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn rot_y(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn shift(x: f64, y: f64, z: f64) -> Mat {
    let mut m = IDENTITY;
    m[3] = [x, y, z, 1.0];
    m
}

/// The joint's motion in A's frame for its values.
pub fn motion(kind: JointKind, q: &[f64]) -> Mat {
    let v = |i: usize| q.get(i).copied().unwrap_or(0.0);
    match kind {
        JointKind::Rigid => IDENTITY,
        JointKind::Revolute => rot_z(v(0)),
        JointKind::Slider => shift(0.0, 0.0, v(0)),
        JointKind::Cylindrical => mat_mul(&rot_z(v(0)), &shift(0.0, 0.0, v(1))),
        JointKind::PinSlot => mat_mul(&shift(v(1), 0.0, 0.0), &rot_z(v(0))),
        JointKind::Planar => mat_mul(&shift(v(0), v(1), 0.0), &rot_z(v(2))),
        JointKind::Ball => mat_mul(&mat_mul(&rot_z(v(0)), &rot_y(v(1))), &rot_z(v(2))),
    }
}

/// Bodies of a component in the model (its own frame).
fn component_bodies<'a>(doc: &Document, st: &'a ModelState, component: u64) -> Vec<&'a crate::ModelBody> {
    st.bodies.iter().filter(|b| doc.body_component(&b.name, b.feature) == component).collect()
}

/// The component an occurrence places (0 for the root).
fn component_of(doc: &Document, occ: u64) -> Result<u64> {
    if occ == 0 {
        return Ok(0);
    }
    doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.component).ok_or_else(|| DocError::Unknown(format!("occurrence {occ}")))
}

/// Resolve a joint origin to a frame in its component's coordinates.
pub fn resolve_origin(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
    let bad = || DocError::Invalid("joint origin: degenerate frame".into());
    match &o.snap {
        Snap::Frame { origin, z, x } => frame(*origin, *z, *x).ok_or_else(bad),
        Snap::Point { point, z } => frame(*point, z.unwrap_or(Vec3::Z), None).ok_or_else(bad),
        Snap::FaceCenter { pick } => {
            let comp = component_of(doc, o.occurrence)?;
            let mut best: Option<(f64, Vec3, Vec3)> = None;
            for b in component_bodies(doc, st, comp) {
                let tol = (b.body.size() * 2e-3).max(1e-3);
                for f in b.body.faces(tol)? {
                    let Some(n) = f.plane_normal else { continue };
                    let d = (*pick - f.centroid).dot(n).abs() + 1e-3 * (*pick - f.centroid).len();
                    if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
                        best = Some((d, f.centroid, n));
                    }
                }
            }
            let (_, c, n) = best.ok_or_else(|| DocError::Invalid("joint origin: no planar face there".into()))?;
            frame(c, n, None).ok_or_else(bad)
        }
        Snap::CircleCenter { pick } => {
            let comp = component_of(doc, o.occurrence)?;
            let cyl = component_bodies(doc, st, comp)
                .into_iter()
                .filter_map(|b| solvecraft_kernel::cylinder_face_at(&b.body, *pick))
                .min_by(|a, b| {
                    let off = |c: &solvecraft_kernel::CylinderFace| {
                        let v = *pick - c.axis_point;
                        ((v - c.axis * v.dot(c.axis)).len() - c.radius).abs()
                    };
                    off(a).total_cmp(&off(b))
                })
                .ok_or_else(|| DocError::Invalid("joint origin: no cylindrical face there".into()))?;
            let s = (*pick - cyl.axis_point).dot(cyl.axis);
            let (at, dir) = if (s - cyl.start).abs() <= (cyl.end - s).abs() { (cyl.start, -1.0) } else { (cyl.end, 1.0) };
            frame(cyl.axis_point + cyl.axis * at, cyl.axis * dir, None).ok_or_else(bad)
        }
    }
}

/// World transform of an occurrence (through its parents).
pub fn occurrence_world(doc: &Document, occ: u64) -> Mat {
    let mut m = IDENTITY;
    let mut cur = doc.occurrences.iter().find(|o| o.id == occ);
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        m = mat_mul(&o.transform, &m);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    m
}

/// Clamp values to the joint's limits.
pub fn clamp(j: &Joint, q: &mut [f64]) {
    for (i, v) in q.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            *v = v.clamp(lo.min(*hi), hi.max(*lo));
        }
    }
}

/// The result of placing occurrences by their joints.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Solution {
    /// New world transforms of occurrences the joints place.
    #[serde(skip)]
    pub world: BTreeMap<u64, Mat>,
    /// Joints that disagree with the placement found through others: (joint id, name, why).
    pub conflicts: Vec<(u64, String, String)>,
    /// Joints that could not be applied (bad origin…): (joint id, name, why).
    pub errors: Vec<(u64, String, String)>,
    /// Remaining degrees of freedom per occurrence (6 = free, 0 = fixed).
    pub dof: BTreeMap<u64, usize>,
    /// Occurrences fixed in place (grounded, or the root's).
    pub fixed: Vec<u64>,
}

/// How far two placements differ: (translation mm, rotation rad).
fn mismatch(a: &Mat, b: &Mat) -> (f64, f64) {
    let t = Vec3::new(a[3][0] - b[3][0], a[3][1] - b[3][1], a[3][2] - b[3][2]).len();
    let mut tr = 0.0;
    for i in 0..3 {
        for k in 0..3 {
            tr += a[i][k] * b[i][k];
        }
    }
    (t, ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos())
}

/// Effective values of every joint after motion links and limits.
pub fn joint_values(asm: &Assembly) -> BTreeMap<u64, Vec<f64>> {
    let mut vals: BTreeMap<u64, Vec<f64>> = asm
        .joints
        .iter()
        .map(|j| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            (j.id, q)
        })
        .collect();
    // Links in order, a few passes for chains.
    for _ in 0..4 {
        for l in &asm.links {
            let Some(src) = vals.get(&l.a).and_then(|q| q.get(l.ia)).copied() else { continue };
            if let Some(v) = vals.get_mut(&l.b).and_then(|q| q.get_mut(l.ib)) {
                *v = l.ratio * src + l.offset;
            }
        }
    }
    for j in &asm.joints {
        if let Some(q) = vals.get_mut(&j.id) {
            clamp(j, q);
        }
    }
    vals
}

/// Place occurrences from the joint graph.
pub fn solve(doc: &Document, st: &ModelState) -> Solution {
    let asm = &doc.assembly;
    let mut sol = Solution::default();
    let joints: Vec<&Joint> = asm.joints.iter().filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    let vals = joint_values(asm);
    // Joint frames in component coordinates (errors leave the joint out).
    let mut frames: BTreeMap<u64, (Mat, Mat)> = BTreeMap::new();
    for j in &joints {
        match (resolve_origin(doc, st, &j.a), resolve_origin(doc, st, &j.b)) {
            (Ok(fa), Ok(fb)) => {
                frames.insert(j.id, (fa, fb));
            }
            (Err(e), _) | (_, Err(e)) => sol.errors.push((j.id, j.name.clone(), e.to_string())),
        }
    }
    // Fixed: the root, grounded occurrences; with neither involved, the first joint's A side.
    let mut placed: BTreeMap<u64, Mat> = BTreeMap::new();
    let mut dof: BTreeMap<u64, usize> = BTreeMap::new();
    placed.insert(0, IDENTITY);
    dof.insert(0, 0);
    for o in doc.occurrences.iter().filter(|o| o.grounded) {
        placed.insert(o.id, occurrence_world(doc, o.id));
        dof.insert(o.id, 0);
    }
    let involved: BTreeSet<u64> = joints.iter().flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    if !involved.iter().any(|o| placed.contains_key(o))
        && let Some(j) = joints.first()
    {
        placed.insert(j.a.occurrence, occurrence_world(doc, j.a.occurrence));
        dof.insert(j.a.occurrence, 0);
    }
    sol.fixed = placed.keys().copied().collect();
    let flip_m = |j: &Joint| {
        let mut m = rot_z(j.angle);
        m[3][2] = j.offset;
        if j.flip { mat_mul(&m, &[[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]) } else { m }
    };
    // B's world transform from A's (or A's from B's, going the other way).
    let through = |j: &Joint, from_a: bool, t: &Mat| -> Option<Mat> {
        let (fa, fb) = frames.get(&j.id)?;
        let q = vals.get(&j.id).cloned().unwrap_or_default();
        let link = mat_mul(&mat_mul(&motion(j.kind, &q), &flip_m(j)), &mat_inverse(fb)?);
        if from_a {
            Some(mat_mul(&mat_mul(t, fa), &link))
        } else {
            // T_A = T_B · link⁻¹ · F_a⁻¹
            Some(mat_mul(&mat_mul(t, &mat_inverse(&link)?), &mat_inverse(fa)?))
        }
    };
    let mut used: BTreeSet<u64> = BTreeSet::new();
    let mut queue: VecDeque<u64> = placed.keys().copied().collect();
    while let Some(occ) = queue.pop_front() {
        let Some(t) = placed.get(&occ).copied() else { continue };
        for j in &joints {
            if used.contains(&j.id) || !frames.contains_key(&j.id) {
                continue;
            }
            let (other, from_a) = if j.a.occurrence == occ {
                (j.b.occurrence, true)
            } else if j.b.occurrence == occ {
                (j.a.occurrence, false)
            } else {
                continue;
            };
            if placed.contains_key(&other) {
                continue;
            }
            let Some(m) = through(j, from_a, &t) else { continue };
            used.insert(j.id);
            placed.insert(other, m);
            dof.insert(other, dof.get(&occ).copied().unwrap_or(0) + j.kind.dofs().len());
            queue.push_back(other);
        }
    }
    // Joints that close loops: do they agree?
    for j in &joints {
        if used.contains(&j.id) || !frames.contains_key(&j.id) {
            continue;
        }
        let (Some(ta), Some(tb)) = (placed.get(&j.a.occurrence), placed.get(&j.b.occurrence)) else { continue };
        let Some(want) = through(j, true, ta) else { continue };
        let (dt, da) = mismatch(&want, tb);
        if dt > 1e-6 || da > 1e-8 {
            sol.conflicts.push((
                j.id,
                j.name.clone(),
                format!("off by {dt:.4} mm and {:.3}° from the placement the other joints give", da.to_degrees()),
            ));
        }
    }
    for o in &doc.occurrences {
        dof.entry(o.id).or_insert(6);
    }
    placed.remove(&0);
    sol.world = placed;
    sol.dof = dof;
    sol
}

impl Document {
    /// Apply a solution: occurrence transforms (in their parents) for the occurrences it places.
    /// Returns true when something moved.
    pub fn apply_joint_solution(&mut self, sol: &Solution) -> bool {
        let mut changed = false;
        let parents: BTreeMap<u64, u64> = self.occurrences.iter().map(|o| (o.id, o.parent)).collect();
        for (occ, world) in &sol.world {
            let parent = parents.get(occ).copied().unwrap_or(0);
            let pw = if parent == 0 { IDENTITY } else { self.component_transform(parent) };
            let Some(local) = mat_inverse(&pw).map(|inv| mat_mul(&inv, world)) else { continue };
            if let Some(o) = self.occurrences.iter_mut().find(|o| o.id == *occ) {
                let (dt, da) = mismatch(&o.transform, &local);
                if dt > 1e-9 || da > 1e-10 {
                    o.transform = local;
                    changed = true;
                }
            }
        }
        changed
    }

    /// A frame for an as-built joint: the world frame `w` seen from an occurrence.
    pub fn frame_in_occurrence(&self, occ: u64, w: &Mat) -> Option<Snap> {
        let t = if occ == 0 { IDENTITY } else { occurrence_world(self, occ) };
        let local = mat_mul(&mat_inverse(&t)?, w);
        let o = apply_point(&local, Vec3::ZERO);
        let z = apply_vector(&local, Vec3::Z);
        let x = apply_vector(&local, Vec3::X);
        Some(Snap::Frame { origin: o, z, x: Some(x) })
    }

    /// A joint origin's frame in the world (for as-built joints and display).
    pub fn origin_world(&self, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
        let f = resolve_origin(self, st, o)?;
        let t = if o.occurrence == 0 { IDENTITY } else { occurrence_world(self, o.occurrence) };
        Ok(mat_mul(&t, &f))
    }
}

/// The frame matrix for an explicit origin / z / x (public for commands).
pub fn frame_of(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    frame(o, z, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Mat, b: &Mat) -> bool {
        let (t, r) = mismatch(a, b);
        t < 1e-9 && r < 1e-9
    }

    #[test]
    fn motions_compose() {
        assert!(close(&motion(JointKind::Revolute, &[std::f64::consts::FRAC_PI_2]), &rot_z(std::f64::consts::FRAC_PI_2)));
        let m = motion(JointKind::Cylindrical, &[0.0, 5.0]);
        assert!((m[3][2] - 5.0).abs() < 1e-12);
        let m = motion(JointKind::PinSlot, &[0.0, 3.0]);
        assert!((m[3][0] - 3.0).abs() < 1e-12);
        assert_eq!(JointKind::Ball.dofs().len(), 3);
        assert!(JointKind::parse("Pin Slot").is_some() && JointKind::parse("nope").is_none());
        let f = frame(Vec3::new(1.0, 2.0, 3.0), Vec3::Z, None).unwrap();
        assert!(close(&mat_mul(&f, &mat_inverse(&f).unwrap()), &IDENTITY));
    }
}
