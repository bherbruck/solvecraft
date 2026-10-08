//! Timeline evaluation.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use solvecraft_geom::{Mesh, Plane, Region2, Vec2, Vec3};
use solvecraft_kernel::{self as kernel, Body, BoolOp};
use solvecraft_sketch::{CurveKind, Profile, SolveReport, find_profiles, solve};

use crate::expr::{self, Kind, Value};
use crate::{AxisRef, DocError, Document, Feature, FeatureKind, HoleKind, Operation, ProfileSel, Result};

/// A body in the evaluated model.
#[derive(Clone, Debug)]
pub struct ModelBody {
    pub name: String,
    pub body: Body,
    /// Feature that created it.
    pub feature: u64,
    mesh: OnceLock<Arc<Mesh>>,
}

impl ModelBody {
    pub fn new(name: String, body: Body, feature: u64) -> Self {
        ModelBody { name, body, feature, mesh: OnceLock::new() }
    }
    /// Display mesh (chord tolerance 1/1000 of the body size), computed once.
    pub fn mesh(&self) -> Arc<Mesh> {
        self.mesh
            .get_or_init(|| {
                let tol = (self.body.size() * 1e-3).max(1e-3);
                Arc::new(self.body.display_mesh(tol).unwrap_or_default())
            })
            .clone()
    }
}

/// A solved sketch in the evaluated model.
#[derive(Clone, Debug)]
pub struct SolvedSketch {
    pub feature: u64,
    pub name: String,
    pub plane: Plane,
    pub sketch: solvecraft_sketch::Sketch,
    pub report: SolveReport,
    pub profiles: Vec<Profile>,
}

/// A cosmetic thread: on which cylinder, its size, and the threaded part of the axis.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThreadInfo {
    pub feature: u64,
    pub body: String,
    pub designation: String,
    pub major_diameter: f64,
    pub pitch: f64,
    pub internal: bool,
    pub axis_point: Vec3,
    pub axis: Vec3,
    /// Threaded part along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
}

/// ISO metric coarse threads: (nominal diameter, pitch).
const ISO_COARSE: [(f64, f64); 22] = [
    (1.6, 0.35),
    (2.0, 0.4),
    (2.5, 0.45),
    (3.0, 0.5),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (36.0, 4.0),
    (42.0, 4.5),
    (48.0, 5.0),
    (56.0, 5.5),
];

/// "M8" or "M8x1" → (nominal diameter, pitch).
pub fn parse_metric_thread(s: &str) -> Option<(f64, f64)> {
    let t = s.trim().to_ascii_uppercase();
    let body = t.strip_prefix('M')?;
    let (d, p) = match body.split_once(['X', '×']) {
        Some((d, p)) => (d.trim().parse::<f64>().ok()?, Some(p.trim().parse::<f64>().ok()?)),
        None => (body.trim().parse::<f64>().ok()?, None),
    };
    let pitch = match p {
        Some(p) => p,
        None => ISO_COARSE.iter().find(|(n, _)| (n - d).abs() < 1e-9)?.1,
    };
    (d > 0.0 && pitch > 0.0 && pitch < d).then_some((d, pitch))
}

/// The metric thread a cylinder of this diameter takes: a shaft is the major diameter, a hole
/// the tap drill (major − pitch).
fn thread_for(diameter: f64, internal: bool) -> Option<(f64, f64)> {
    ISO_COARSE
        .iter()
        .map(|&(n, p)| (n, p, if internal { n - p } else { n }))
        .min_by(|a, b| (a.2 - diameter).abs().total_cmp(&(b.2 - diameter).abs()))
        .filter(|(_, _, want)| (want - diameter).abs() <= 0.15 * diameter)
        .map(|(n, p, _)| (n, p))
}

fn format_thread(d: f64, p: f64) -> String {
    let coarse = ISO_COARSE.iter().any(|(n, q)| (n - d).abs() < 1e-9 && (q - p).abs() < 1e-9);
    let num = |x: f64| if (x - x.round()).abs() < 1e-9 { format!("{}", x.round()) } else { format!("{x}") };
    if coarse { format!("M{}", num(d)) } else { format!("M{}x{}", num(d), num(p)) }
}

/// A cosmetic thread on the cylindrical face at `p`.
fn thread_on(st: &ModelState, feature: u64, p: Vec3, designation: Option<&str>, length: Option<f64>) -> Result<ThreadInfo> {
    let (body, cyl) = st
        .bodies
        .iter()
        .filter_map(|b| kernel::cylinder_face_at(&b.body, p).map(|c| (b.name.clone(), c)))
        .min_by(|a, b| {
            let dist = |c: &kernel::CylinderFace| ((p - c.axis_point - c.axis * (p - c.axis_point).dot(c.axis)).len() - c.radius).abs();
            dist(&a.1).total_cmp(&dist(&b.1))
        })
        .ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
    let dia = cyl.radius * 2.0;
    let (major, pitch) = match designation {
        Some(s) => parse_metric_thread(s).ok_or_else(|| DocError::Invalid(format!("unknown thread `{s}` (ISO metric, like M8 or M8x1)")))?,
        None => thread_for(dia, cyl.internal).ok_or_else(|| DocError::Invalid(format!("no metric thread fits a {dia:.2} mm cylinder")))?,
    };
    let fits = if cyl.internal {
        (major - pitch * 1.0825 - dia).abs() < 0.2 * major || (major - pitch - dia).abs() < 0.15 * major
    } else {
        (major - dia).abs() < 0.15 * major
    };
    if !fits {
        return Err(DocError::Invalid(format!(
            "an {} thread does not fit a {dia:.2} mm {}",
            format_thread(major, pitch),
            if cyl.internal { "hole" } else { "shaft" }
        )));
    }
    // From the end of the face nearest the point.
    let s = (p - cyl.axis_point).dot(cyl.axis);
    let (mut a, mut b) = (cyl.start, cyl.end);
    if let Some(l) = length {
        if !(l > 0.0) {
            return Err(DocError::Invalid("thread length must be positive".into()));
        }
        if (s - a).abs() <= (b - s).abs() {
            b = (a + l).min(b);
        } else {
            a = (b - l).max(a);
        }
    }
    Ok(ThreadInfo {
        feature,
        body,
        designation: format_thread(major, pitch),
        major_diameter: major,
        pitch,
        internal: cyl.internal,
        axis_point: cyl.axis_point,
        axis: cyl.axis,
        start: a,
        end: b,
    })
}

/// Model after some prefix of the timeline.
#[derive(Clone, Debug, Default)]
pub struct ModelState {
    /// Cosmetic threads (from Thread features and threaded holes).
    pub threads: Vec<ThreadInfo>,
    pub bodies: Vec<ModelBody>,
    pub sketches: Vec<SolvedSketch>,
    pub body_counter: usize,
    /// Sheet metal bodies (by body name) and their flat patterns.
    pub sheets: Vec<crate::sheet::SheetBody>,
}

impl ModelState {
    pub fn body(&self, name: &str) -> Option<&ModelBody> {
        self.bodies.iter().find(|b| b.name == name)
    }
    pub fn sketch(&self, feature: u64) -> Option<&SolvedSketch> {
        self.sketches.iter().find(|s| s.feature == feature)
    }
}

/// Evaluation result of one feature.
#[derive(Clone, Debug, Serialize)]
pub struct FeatureResult {
    pub id: u64,
    pub name: String,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub ms: f64,
    pub skipped: bool,
    #[serde(skip)]
    fingerprint: u64,
    #[serde(skip)]
    pub state: Arc<ModelState>,
}

/// Incremental evaluator: holds the state after every feature.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub results: Vec<FeatureResult>,
    pub param_errors: BTreeMap<String, String>,
    empty: Arc<ModelState>,
    /// Features recomputed by the last `evaluate`.
    pub last_recomputed: usize,
}

/// Parameters a feature uses directly (inputs and sketch dimensions), sorted.
fn param_names(f: &Feature) -> Vec<String> {
    let mut names: Vec<String> = f.kind.expressions().iter().flat_map(|e| expr::references(e)).collect();
    if let FeatureKind::Sketch { sketch, .. } = &f.kind {
        names.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
    }
    names.sort();
    names.dedup();
    names
}

fn fingerprint(prev: u64, f: &Feature, vals: &BTreeMap<String, Value>, rolled_back: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prev.hash(&mut h);
    rolled_back.hash(&mut h);
    serde_json::to_string(f).unwrap_or_default().hash(&mut h);
    for n in param_names(f) {
        n.hash(&mut h);
        match vals.get(&n) {
            Some(v) => {
                v.v.to_bits().hash(&mut h);
                v.len.hash(&mut h);
                v.ang.hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

fn now() -> web_time::Instant {
    web_time::Instant::now()
}

impl Model {
    pub fn new() -> Self {
        Model::default()
    }

    /// The final model state.
    pub fn state(&self) -> Arc<ModelState> {
        self.results.iter().rev().find(|r| !r.skipped).map(|r| r.state.clone()).unwrap_or_else(|| self.empty.clone())
    }

    /// State just before the feature with this id (for editing a sketch in context).
    pub fn state_before(&self, id: u64) -> Arc<ModelState> {
        let Some(i) = self.results.iter().position(|r| r.id == id) else { return self.state() };
        self.results.get(..i).and_then(|s| s.iter().rev().find(|r| !r.skipped)).map(|r| r.state.clone()).unwrap_or_else(|| self.empty.clone())
    }

    pub fn result(&self, id: u64) -> Option<&FeatureResult> {
        self.results.iter().find(|r| r.id == id)
    }

    /// Re-evaluate the document, reusing results up to the first changed feature.
    pub fn evaluate(&mut self, doc: &Document) {
        let (vals, perr) = doc.param_values();
        self.param_errors = perr;
        let mut prev_fp = 0u64;
        let mut state = self.empty.clone();
        let mut out: Vec<FeatureResult> = Vec::with_capacity(doc.features.len());
        let mut reuse = true;
        let mut recomputed = 0;
        let marker = doc.marker.unwrap_or(usize::MAX);
        for (i, f) in doc.features.iter().enumerate() {
            let rolled_back = i >= marker;
            let mut fp = fingerprint(prev_fp, f, &vals, rolled_back);
            // Sheet metal features also depend on the rules (and the parameters they use).
            if f.kind.is_sheet() {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                fp.hash(&mut h);
                serde_json::to_string(&doc.sheet).unwrap_or_default().hash(&mut h);
                for r in &doc.sheet.rules {
                    for e in [&r.thickness, &r.k_factor, &r.bend_radius, &r.relief_width, &r.relief_depth, &r.corner_relief, &r.hem_gap, &r.gap] {
                        for n in expr::references(e) {
                            vals.get(&n).map(|v| v.v.to_bits()).hash(&mut h);
                        }
                    }
                }
                fp = h.finish();
            }
            prev_fp = fp;
            if reuse
                && let Some(old) = self.results.get(i)
                && old.id == f.id
                && old.fingerprint == fp
            {
                if !old.skipped {
                    state = old.state.clone();
                }
                out.push(old.clone());
                continue;
            }
            reuse = false;
            if f.suppressed || rolled_back {
                out.push(FeatureResult {
                    id: f.id,
                    name: f.name.clone(),
                    error: None,
                    warning: None,
                    ms: 0.0,
                    skipped: true,
                    fingerprint: fp,
                    state: state.clone(),
                });
                continue;
            }
            recomputed += 1;
            let t0 = now();
            let mut next = (*state).clone();
            let mut warning = None;
            // A parameter with an error fails the features using it, with that error.
            let bad_param = param_names(f).into_iter().find_map(|n| self.param_errors.get(&n).map(|e| format!("parameter `{n}`: {e}")));
            let r = match bad_param {
                Some(e) => Err(kernel::KernelError::Failed(e)),
                None => kernel::guard(&f.name, || {
                    eval_feature(doc, &vals, f, &mut next, &mut warning).map_err(|e| kernel::KernelError::Failed(e.to_string()))
                }),
            };
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let error = match r {
                Ok(()) => {
                    state = Arc::new(next);
                    None
                }
                Err(e) => Some(e.to_string().trim_start_matches("the operation failed: ").to_string()),
            };
            out.push(FeatureResult { id: f.id, name: f.name.clone(), error, warning, ms, skipped: false, fingerprint: fp, state: state.clone() });
        }
        self.results = out;
        self.last_recomputed = recomputed;
    }
}

fn val(vals: &BTreeMap<String, Value>, e: &str, k: Kind) -> Result<f64> {
    Document::eval_in(vals, e, k)
}

fn select_profiles(ss: &SolvedSketch, sel: &ProfileSel) -> Result<Vec<Region2>> {
    let ps = &ss.profiles;
    if ps.is_empty() {
        return Err(DocError::Invalid(format!("{} has no closed profiles", ss.name)));
    }
    let out: Vec<Region2> = match sel {
        // "All" means what was drawn: projected reference geometry (a face's auto-projected
        // edges) only counts when the drawn curves alone make no profile.
        ProfileSel::All => {
            let drawn = solvecraft_sketch::find_drawn_profiles(&ss.sketch);
            if drawn.is_empty() { ps.iter().map(|p| p.region.clone()).collect() } else { drawn.into_iter().map(|p| p.region).collect() }
        }
        ProfileSel::Indices { indices } => {
            let mut v = Vec::new();
            for i in indices {
                v.push(ps.get(*i).ok_or_else(|| DocError::Unknown(format!("profile {i} of {}", ss.name)))?.region.clone());
            }
            v
        }
        ProfileSel::Curves { loops } => {
            let mut v = Vec::new();
            for l in loops {
                let mut want: Vec<&String> = l.iter().collect();
                want.sort();
                let found = ps.iter().find(|p| {
                    let mut have: Vec<&String> = p.outer_curves.iter().collect();
                    have.sort();
                    have == want
                });
                v.push(found.ok_or_else(|| DocError::Unknown(format!("profile bounded by {l:?} in {}", ss.name)))?.region.clone());
            }
            v
        }
        ProfileSel::Points { points } => {
            let mut v = Vec::new();
            for p in points {
                // Innermost profile containing the point.
                let found = ps.iter().filter(|q| q.region.contains(*p)).min_by(|a, b| a.area.total_cmp(&b.area));
                v.push(found.ok_or_else(|| DocError::Unknown(format!("profile at {:?} in {}", [p.x, p.y], ss.name)))?.region.clone());
            }
            v
        }
    };
    if out.is_empty() {
        return Err(DocError::Invalid("no profile selected".into()));
    }
    Ok(out)
}

fn new_name(state: &mut ModelState, f: &Feature, k: usize) -> String {
    if let Some(n) = f.body_names.get(k)
        && !n.is_empty()
        && state.body(n).is_none()
    {
        return n.clone();
    }
    loop {
        state.body_counter += 1;
        let n = format!("Body{}", state.body_counter);
        if state.body(&n).is_none() {
            return n;
        }
    }
}

/// Add tool bodies to the model according to the operation.
fn apply_op(state: &mut ModelState, f: &Feature, tools: Vec<Body>, op: Operation, targets: &[String]) -> Result<()> {
    let target_idx: Vec<usize> = if targets.is_empty() {
        (0..state.bodies.len()).collect()
    } else {
        let mut v = Vec::new();
        for t in targets {
            v.push(state.bodies.iter().position(|b| &b.name == t).ok_or_else(|| DocError::Unknown(format!("body `{t}`")))?);
        }
        v
    };
    if op == Operation::NewBody || target_idx.is_empty() {
        if op == Operation::Cut || op == Operation::Intersect {
            return Err(DocError::Invalid("there is no body to cut or intersect".into()));
        }
        for (k, b) in tools.into_iter().enumerate() {
            let name = new_name(state, f, k);
            state.bodies.push(ModelBody::new(name, b, f.id));
        }
        return Ok(());
    }
    match op {
        Operation::Join => {
            let Some(&first) = target_idx.first() else { return Ok(()) };
            let Some(mut acc) = state.bodies.get(first).map(|b| b.body.clone()) else { return Ok(()) };
            for t in &tools {
                acc = kernel::boolean(&acc, t, BoolOp::Union)?.ok_or_else(|| DocError::Invalid("join produced nothing".into()))?;
            }
            if let Some(mb) = state.bodies.get_mut(first) {
                *mb = ModelBody::new(mb.name.clone(), acc, mb.feature);
            }
        }
        Operation::Cut | Operation::Intersect => {
            let bop = if op == Operation::Cut { BoolOp::Cut } else { BoolOp::Intersect };
            let mut remove = Vec::new();
            for &ti in &target_idx {
                let Some(mut acc) = state.bodies.get(ti).map(|b| Some(b.body.clone())) else { continue };
                for t in &tools {
                    acc = match acc {
                        Some(a) => kernel::boolean(&a, t, bop)?,
                        None => None,
                    };
                }
                match acc {
                    Some(b) => {
                        if let Some(mb) = state.bodies.get_mut(ti) {
                            *mb = ModelBody::new(mb.name.clone(), b, mb.feature);
                        }
                    }
                    None => remove.push(ti),
                }
            }
            let mut i = 0;
            state.bodies.retain(|_| {
                let keep = !remove.contains(&i);
                i += 1;
                keep
            });
            // A cut can leave a body in pieces: each piece becomes its own body ("Bar", "Bar (1)").
            let mut split: Vec<ModelBody> = Vec::new();
            for mb in std::mem::take(&mut state.bodies) {
                let pieces = mb.body.lumps().unwrap_or_else(|_| vec![mb.body.clone()]);
                if pieces.len() < 2 {
                    split.push(mb);
                    continue;
                }
                for (k, p) in pieces.into_iter().enumerate() {
                    let name = if k == 0 { mb.name.clone() } else { format!("{} ({k})", mb.name) };
                    split.push(ModelBody::new(name, p, mb.feature));
                }
            }
            state.bodies = split;
        }
        Operation::NewBody => {}
    }
    Ok(())
}

/// Distance that reaches through every target body from the sketch plane (either side).
fn through_all_distance(st: &ModelState, plane: &Plane, targets: &[String]) -> Result<f64> {
    let mut far: f64 = 0.0;
    let mut any = false;
    for b in st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)) {
        let bb = b.mesh().bounds();
        if bb.is_empty() {
            continue;
        }
        any = true;
        for i in 0..8 {
            let p = Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            far = far.max(plane.height(p).abs());
        }
    }
    if !any {
        return Err(DocError::Invalid("through all needs a body to cut".into()));
    }
    Ok(far * 1.02 + 1.0)
}

/// Sample points inside a region (sketch coordinates).
fn region_samples(r: &Region2) -> Vec<Vec2> {
    let mut out = vec![r.interior_point()];
    let pts = r.outer.polyline(1e-2);
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in &pts {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    for i in 0..5 {
        for j in 0..5 {
            let p = Vec2::new(lo.x + (hi.x - lo.x) * (0.1 + 0.2 * i as f64), lo.y + (hi.y - lo.y) * (0.1 + 0.2 * j as f64));
            if r.contains(p) {
                out.push(p);
            }
        }
    }
    out
}

/// Grow profile edges that lie on a face of a target body away from the profile, by a small
/// amount, when the space they grow into is already material (join) or empty (cut). `probe`
/// maps a sketch point to the world points where the tool would be (along the extrude, around
/// the revolve). Only loops of straight edges are adjusted.
fn grow_profile_for_coplanar(st: &ModelState, r: &Region2, op: Operation, targets: &[String], probe: &dyn Fn(Vec2) -> Vec<Vec3>) -> Region2 {
    if !matches!(op, Operation::Cut | Operation::Join) {
        return r.clone();
    }
    let bodies: Vec<&ModelBody> = st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).collect();
    if bodies.is_empty() {
        return r.clone();
    }
    let meshes: Vec<Arc<Mesh>> = bodies.iter().map(|b| b.mesh()).collect();
    let size = bodies.iter().map(|b| b.body.size()).fold(0.0, f64::max);
    let delta = (size * 0.005).max(1e-3);
    let lp = r.outer.ccw();
    let lines: Option<Vec<(Vec2, Vec2)>> =
        lp.segs.iter().map(|s| if let solvecraft_geom::Seg2::Line { a, b } = *s { Some((a, b)) } else { None }).collect();
    let Some(lines) = lines else { return r.clone() };
    let inside_any = |p: Vec3| meshes.iter().any(|m| m.contains(p));
    let mut grow = vec![0.0; lines.len()];
    let mut any = false;
    for (i, (a, b)) in lines.iter().enumerate() {
        let Some(dir) = (*b - *a).normalized() else { continue };
        let out = Vec2::new(dir.y, -dir.x);
        // Is the edge on a body face? Material on exactly one side of it.
        let mids = [a.lerp(*b, 0.25), a.lerp(*b, 0.5), a.lerp(*b, 0.75)];
        let side = |k: f64| mids.iter().flat_map(|m| probe(*m + out * (delta * k))).map(inside_any).collect::<Vec<bool>>();
        let (outer, inner) = (side(0.5), side(-0.5));
        let want_outer = op == Operation::Join;
        let on_face = outer.iter().all(|x| *x == want_outer) && inner.iter().any(|x| *x != want_outer);
        let room = side(1.0).iter().all(|x| *x == want_outer);
        if on_face && room {
            if let Some(g) = grow.get_mut(i) {
                *g = delta;
            }
            any = true;
        }
    }
    if !any {
        return r.clone();
    }
    // Offset the chosen lines and re-intersect neighbours.
    let n = lines.len();
    let shifted: Vec<(Vec2, Vec2)> = lines
        .iter()
        .zip(&grow)
        .map(|((a, b), g)| {
            let dir = (*b - *a).normalized().unwrap_or(Vec2::X);
            let out = Vec2::new(dir.y, -dir.x);
            (*a + out * *g, dir)
        })
        .collect();
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(&(p0, d0)), Some(&(p1, d1))) = (shifted.get((i + n - 1) % n), shifted.get(i)) else { return r.clone() };
        let den = d0.cross(d1);
        if den.abs() < 1e-12 {
            pts.push(p1);
            continue;
        }
        let t = (p1 - p0).cross(d1) / den;
        pts.push(p0 + d0 * t);
    }
    Region2 { outer: solvecraft_geom::Loop2::polygon(&pts), holes: r.holes.clone() }
}

/// Booleans struggle with coincident faces (a cut starting on the face it cuts, a boss
/// starting on the face it joins). Extending the tool past such an end changes nothing when the
/// space beyond is empty (cut) or already material (join), so do exactly that.
fn extend_for_coplanar(st: &ModelState, plane: &Plane, r: &Region2, lo: f64, hi: f64, op: Operation, targets: &[String]) -> (f64, f64) {
    if !matches!(op, Operation::Cut | Operation::Join) || st.bodies.is_empty() {
        return (lo, hi);
    }
    let bodies: Vec<&ModelBody> = st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).collect();
    if bodies.is_empty() {
        return (lo, hi);
    }
    let size = bodies.iter().map(|b| b.body.size()).fold(0.0, f64::max);
    let delta = (size * 0.01).max(1e-3).min((hi - lo).abs().max(1e-3));
    let meshes: Vec<Arc<Mesh>> = bodies.iter().map(|b| b.mesh()).collect();
    let samples = region_samples(r);
    let n = plane.normal();
    let inside_any = |p: Vec3| meshes.iter().any(|m| m.contains(p));
    let ok_beyond = |h: f64, dir: f64| {
        samples.iter().all(|s| {
            let base = plane.to_world(*s);
            [0.5, 1.0].iter().all(|k| {
                let p = base + n * (h + dir * delta * k);
                let inside = inside_any(p);
                if op == Operation::Cut { !inside } else { inside }
            })
        })
    };
    let lo2 = if ok_beyond(lo, -1.0) { lo - delta } else { lo };
    let hi2 = if ok_beyond(hi, 1.0) { hi + delta } else { hi };
    (lo2, hi2)
}

/// The body a face-based feature applies to: by name, or the one whose surface is nearest the
/// first point.
fn body_at(st: &ModelState, body: &Option<String>, points: &[Vec3]) -> Result<usize> {
    if let Some(n) = body {
        return st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")));
    }
    let p = points.first().ok_or_else(|| DocError::Invalid("no faces selected".into()))?;
    st.bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let bb = b.mesh().bounds();
            let q = Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z));
            (i, q.dist(*p))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| DocError::Invalid("there is no body".into()))
}

/// A hole stays on its face when the model changes: a position now inside a body moves back
/// against the drilling direction to the surface, one now above it moves down onto it.
fn hole_on_surface(st: &ModelState, p: Vec3, dir: Vec3) -> Vec3 {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let eps = size * 1e-6;
    let mut best: Option<Vec3> = None;
    for b in &st.bodies {
        let m = b.mesh();
        // Already on the surface: the drill enters right here.
        if let Some((t, _)) = m.raycast(p - dir * eps, dir)
            && t < eps * 4.0
        {
            return p;
        }
        let q = if m.contains(p) { m.raycast(p, -dir).map(|(t, _)| p - dir * t) } else { m.raycast(p, dir).map(|(t, _)| p + dir * t) };
        if let Some(q) = q
            && best.is_none_or(|bq| q.dist(p) < bq.dist(p))
        {
            best = Some(q);
        }
    }
    best.unwrap_or(p)
}

/// Edge reference points re-found on the body: points on an edge stay; a point that moved off
/// (an upstream edit) goes to the nearest edge with a warning; points with no edge near are
/// dropped with a warning.
fn resolve_edges(b: &Body, pts: &[Vec3], warning: &mut Option<String>) -> Result<Vec<Vec3>> {
    let size = b.size();
    let edges = b.edges((size * 1e-3).max(1e-3))?;
    let tight = (size * 2e-3).max(1e-3);
    let (mut out, mut moved, mut lost) = (Vec::new(), 0, 0);
    for p in pts {
        let best = edges
            .iter()
            .map(|e| (e.points.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min), e.mid))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((d, _)) if d <= tight => out.push(*p),
            Some((d, mid)) if d <= size * 0.25 => {
                out.push(mid);
                moved += 1;
            }
            _ => lost += 1,
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("none of the selected edges exist any more".into()));
    }
    if moved + lost > 0 {
        let mut w = Vec::new();
        if moved > 0 {
            w.push(format!("{moved} edge reference(s) re-found on the nearest edge"));
        }
        if lost > 0 {
            w.push(format!("{lost} edge reference(s) no longer found and skipped"));
        }
        *warning = Some(w.join("; "));
    }
    Ok(out)
}

/// The body a fillet/chamfer applies to: by name, or the one closest to the first edge point.
fn blend_target(state: &ModelState, body: &Option<String>, edges: &[Vec3]) -> Result<usize> {
    if let Some(n) = body {
        return state.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")));
    }
    let p = edges.first().ok_or_else(|| DocError::Invalid("no edges selected".into()))?;
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in state.bodies.iter().enumerate() {
        let tol = (b.body.size() * 2e-3).max(1e-3);
        if let Ok(Some((_, d))) = b.body.nearest_edge(*p, tol)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i).ok_or_else(|| {
        if state.bodies.iter().any(|b| b.body.is_mesh()) {
            DocError::Invalid("there is no solid body to modify (mesh bodies have no edges)".into())
        } else {
            DocError::Invalid("there is no body to modify".into())
        }
    })
}

fn revolve_axis(ss: &SolvedSketch, axis: &AxisRef) -> Result<(Vec2, Vec2)> {
    match axis {
        AxisRef::SketchLine { curve } => {
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("sketch line `{curve}`")))?;
            match ss.sketch.curves.get(ci).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => {
                    let (pa, pb) = (ss.sketch.point(*a).unwrap_or_default(), ss.sketch.point(*b).unwrap_or_default());
                    Ok((pa, pb - pa))
                }
                _ => Err(DocError::Invalid(format!("`{curve}` is not a line"))),
            }
        }
        AxisRef::SketchAxis { axis } => match axis.to_ascii_lowercase().as_str() {
            "x" => Ok((Vec2::ZERO, Vec2::X)),
            "y" => Ok((Vec2::ZERO, Vec2::Y)),
            other => Err(DocError::Unknown(format!("sketch axis `{other}`"))),
        },
        AxisRef::World { axis } => {
            let d = match axis.to_ascii_uppercase().as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                "Z" => Vec3::Z,
                other => return Err(DocError::Unknown(format!("axis `{other}`"))),
            };
            let pl = &ss.plane;
            if d.dot(pl.normal()).abs() > 1e-9 || pl.height(Vec3::ZERO).abs() > 1e-9 {
                return Err(DocError::Invalid(format!("the {axis} axis does not lie in the sketch plane")));
            }
            Ok((pl.to_local(Vec3::ZERO), Vec2::new(d.dot(pl.x), d.dot(pl.y))))
        }
        AxisRef::Line { origin, dir } => {
            let pl = &ss.plane;
            let d = dir.normalized().ok_or_else(|| DocError::Invalid("revolve axis has no direction".into()))?;
            if d.dot(pl.normal()).abs() > 1e-9 || pl.height(*origin).abs() > 1e-6 {
                return Err(DocError::Invalid("the revolve axis does not lie in the sketch plane".into()));
            }
            Ok((pl.to_local(*origin), Vec2::new(d.dot(pl.x), d.dot(pl.y))))
        }
    }
}

/// Where a hole feature drills (each on the surface) and in which direction: at its sketch
/// points perpendicular to the sketch (into the material), else at its position.
fn hole_spots(st: &ModelState, f: &Feature) -> Result<(Vec<Vec3>, Vec3)> {
    let FeatureKind::Hole { position, direction, points, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    let (spots, dir) = match points {
        Some(sp) => {
            let ss = st.sketch(sp.sketch).ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sp.sketch)))?;
            let mut v = Vec::new();
            for id in &sp.ids {
                let p = ss.sketch.points.iter().find(|p| &p.id == id).ok_or_else(|| DocError::Unknown(format!("sketch point `{id}`")))?;
                v.push(ss.plane.to_world(p.pos));
            }
            (v, -ss.plane.normal())
        }
        None => (vec![*position], direction.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?),
    };
    if spots.is_empty() || spots.len() > 10_000 {
        return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
    }
    Ok((spots.into_iter().map(|p| hole_on_surface(st, p, dir)).collect(), dir))
}

/// The cutting tools of one hole at `position`, drilling along `dir`.
#[allow(clippy::too_many_arguments)]
fn hole_tools(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &Vec3,
    dir: Vec3,
    diameter: &str,
    depth: &Option<String>,
    hole: &HoleKind,
) -> Result<Vec<Body>> {
    let r = val(vals, diameter, Kind::Length)? / 2.0;
    if !(r > 1e-6) {
        return Err(DocError::Invalid("hole diameter must be positive".into()));
    }
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let top = (size * 0.01).max(0.1);
    let bottom = match depth {
        Some(d) => -val(vals, d, Kind::Length)?,
        None => -(size * 2.0 + 1.0),
    };
    if bottom >= 0.0 {
        return Err(DocError::Invalid("hole depth must be positive".into()));
    }
    // Tools are cylinders and cones made by (tapered) extrudes along the axis, cut one
    // after another, widest first.
    let plane = Plane::from_normal(*position, -dir).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
    // Each tool's circle seam at its own angle, so seams don't line up between tools.
    let seam = std::cell::Cell::new(0.0);
    let disc = |rad: f64| {
        seam.set(seam.get() + 0.613);
        Region2 { outer: solvecraft_geom::Loop2::circle_from(Vec2::ZERO, rad, seam.get()), holes: vec![] }
    };
    let cyl = |rad: f64, lo: f64, hi: f64| -> Result<Body> {
        kernel::extrude(&plane, &[disc(rad)], lo, hi)?.pop().ok_or_else(|| DocError::Invalid("hole tool".into()))
    };
    let mut tools = Vec::new();
    match hole {
        HoleKind::Simple => tools.push(cyl(r, bottom, top)?),
        HoleKind::Drilled { tip_angle } => {
            let half = val(vals, tip_angle, Kind::Angle)? / 2.0;
            if depth.is_some() && half > 1e-3 && half < std::f64::consts::FRAC_PI_2 {
                // One revolved tool (cylinder with a drill point).
                let tip = r / half.tan();
                let up = -dir;
                let side = up.any_perp();
                let rp = Plane::new(*position, side, up).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
                let pts = [Vec2::new(0.0, top), Vec2::new(r, top), Vec2::new(r, bottom), Vec2::new(0.0, bottom - tip)];
                let region = Region2 { outer: solvecraft_geom::Loop2::polygon(&pts), holes: vec![] };
                tools.extend(kernel::revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?);
            } else {
                tools.push(cyl(r, bottom, top)?);
            }
        }
        HoleKind::Counterbore { cb_diameter, cb_depth } => {
            let (rc, dc) = (val(vals, cb_diameter, Kind::Length)? / 2.0, val(vals, cb_depth, Kind::Length)?);
            if !(rc > r && dc > 0.0 && -dc > bottom) {
                return Err(DocError::Invalid("the counterbore must be wider than the hole and shallower than it".into()));
            }
            // Wide and shallow first: the narrow hole then crosses the pocket floor.
            tools.push(cyl(rc, -dc, top)?);
            tools.push(cyl(r, bottom, top)?);
        }
        HoleKind::Countersink { cs_diameter, cs_angle } => {
            let (rc, half) = (val(vals, cs_diameter, Kind::Length)? / 2.0, val(vals, cs_angle, Kind::Angle)? / 2.0);
            if !(rc > r && half > 1e-3 && half < std::f64::consts::FRAC_PI_2) {
                return Err(DocError::Invalid("the countersink must be wider than the hole".into()));
            }
            // Cone from above the face to just inside the hole radius, then the hole.
            let r_top = rc + top * half.tan();
            let r_end = r * 0.9;
            let len = (r_top - r_end) / half.tan();
            let top_plane = plane.offset(top);
            tools.push(kernel::extrude_tapered(&top_plane, &disc(r_top), len, -1.0, -half)?);
            tools.push(cyl(r, bottom, top)?);
        }
    }
    Ok(tools)
}

/// The tool bodies a feature adds or removes (extrude, revolve, primitives), in the current state.
fn feature_tools(vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Vec<Body>> {
    match &f.kind {
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?.clone();
            let regions = solvecraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let d = if extent.through_all { through_all_distance(st, &ss.plane, targets)? } else { val(vals, &extent.distance, Kind::Length)? };
            if let Some(tp) = &extent.taper {
                let taper = val(vals, tp, Kind::Angle)?;
                if extent.distance2.is_some() || extent.start_offset.is_some() || extent.direction == crate::Direction::Symmetric {
                    return Err(DocError::Invalid("not supported yet: taper with two-sided, symmetric or offset extents".into()));
                }
                let sign = if extent.direction == crate::Direction::Negative { -1.0 } else { 1.0 };
                let (len, sign) = if d < 0.0 { (-d, -sign) } else { (d, sign) };
                return Ok(regions
                    .iter()
                    .map(|r| kernel::extrude_tapered(&ss.plane, r, len, sign, taper))
                    .collect::<std::result::Result<Vec<_>, _>>()?);
            }
            let off = match &extent.start_offset {
                Some(e) => val(vals, e, Kind::Length)?,
                None => 0.0,
            };
            let (a, b) = match (&extent.distance2, extent.direction) {
                (Some(d2), _) => (-val(vals, d2, Kind::Length)?, d),
                (None, crate::Direction::Positive) => (0.0, d),
                (None, crate::Direction::Negative) => (-d, 0.0),
                (None, crate::Direction::Symmetric) => (-d, d),
            };
            // A negative distance flips the side.
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let mut tools = Vec::new();
            for r in &regions {
                let (l2, h2) = extend_for_coplanar(st, &ss.plane, r, lo + off, hi + off, *operation, targets);
                let n = ss.plane.normal();
                let probe = |p: Vec2| (1..4).map(|k| ss.plane.to_world(p) + n * (lo + off + (hi - lo) * k as f64 / 4.0)).collect::<Vec<_>>();
                let r = grow_profile_for_coplanar(st, r, *operation, targets, &probe);
                tools.extend(kernel::extrude(&ss.plane, std::slice::from_ref(&r), l2, h2)?);
            }
            Ok(tools)
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, .. } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?.clone();
            let regions = solvecraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let (o, d) = revolve_axis(&ss, axis)?;
            let ang = val(vals, angle, Kind::Angle)?;
            let (operation, targets) = feature_op(f);
            let axis_w = ss.plane.dir_to_world(d);
            let origin_w = ss.plane.to_world(o);
            let probe = |p: Vec2| {
                let q = ss.plane.to_world(p);
                (1..4)
                    .map(|k| {
                        let a = ang * k as f64 / 4.0;
                        let v = q - origin_w;
                        origin_w + v * a.cos() + axis_w.cross(v) * a.sin() + axis_w * (axis_w.dot(v) * (1.0 - a.cos()))
                    })
                    .collect::<Vec<_>>()
            };
            let regions: Vec<Region2> = regions.iter().map(|r| grow_profile_for_coplanar(st, r, operation, &targets, &probe)).collect();
            Ok(kernel::revolve(&ss.plane, &regions, o, d, ang)?)
        }
        FeatureKind::Loft { sections, .. } if sections.iter().any(|s| s.point.is_some()) => {
            // A profile lofted to a sketch point (an apex).
            let [a, b] = &sections[..] else { return Err(DocError::Invalid("not supported yet: a point section in a loft of more than two".into())) };
            let (prof, pt) = if a.point.is_some() { (b, a) } else { (a, b) };
            let ss = st.sketch(prof.sketch).ok_or_else(|| DocError::Unknown(format!("sketch {}", prof.sketch)))?;
            let regions = solvecraft_sketch::merge_regions(&select_profiles(ss, &prof.profiles)?);
            let [r] = &regions[..] else { return Err(DocError::Invalid("each loft section must be one profile".into())) };
            let ps = st.sketch(pt.sketch).ok_or_else(|| DocError::Unknown(format!("sketch {}", pt.sketch)))?;
            let id = pt.point.as_deref().unwrap_or_default();
            let q = ps.sketch.points.iter().find(|p| p.id == id).ok_or_else(|| DocError::Unknown(format!("point `{id}` in {}", ps.name)))?;
            Ok(vec![kernel::loft_to_point(&ss.plane, &r.outer, ps.plane.to_world(q.pos))?])
        }
        FeatureKind::Coil { base, axis, diameter, pitch, turns, section_size, section, position, start_angle, clockwise, .. } => {
            let k = axis.normalized().ok_or_else(|| DocError::Invalid("coil axis".into()))?;
            let (d, p, n, s) = (
                val(vals, diameter, Kind::Length)?,
                val(vals, pitch, Kind::Length)?,
                val(vals, turns, Kind::Unitless)?,
                val(vals, section_size, Kind::Length)?,
            );
            if !(d > 0.0 && s > 0.0 && n > 0.0) {
                return Err(DocError::Invalid("the coil diameter, section size and revolutions must be positive".into()));
            }
            let r = d / 2.0 + f64::from(*position) * s / 2.0;
            if r <= s / 2.0 {
                return Err(DocError::Invalid("the section reaches the coil's axis".into()));
            }
            let a0 = match start_angle {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => 0.0,
            };
            // Radial direction at the start; the section lies in the plane of the axis.
            let x0 = if k.cross(Vec3::X).len() > 1e-6 { (Vec3::X - k * k.dot(Vec3::X)).normalized() } else { Some(k.any_perp()) };
            let x0 = x0.ok_or_else(|| DocError::Invalid("coil axis".into()))?;
            let x = x0 * a0.cos() + k.cross(x0) * a0.sin();
            let plane = Plane { origin: *base, x, y: k };
            let c = Vec2::new(r, 0.0);
            let outer = match section {
                crate::CoilSection::Circular => solvecraft_geom::Loop2::circle(c, s / 2.0),
                crate::CoilSection::Square => {
                    let h = s / 2.0;
                    solvecraft_geom::Loop2::polygon(&[c + Vec2::new(-h, -h), c + Vec2::new(h, -h), c + Vec2::new(h, h), c + Vec2::new(-h, h)])
                }
            };
            let region = Region2 { outer, holes: Vec::new() };
            let axis = if *clockwise { k * -1.0 } else { k };
            // A left-handed coil turns about −axis and climbs along +axis: mirror the pitch.
            let pitch = if *clockwise { -p } else { p };
            Ok(vec![kernel::sweep_helix(&plane, &region, *base, axis, pitch, n)?])
        }
        FeatureKind::Loft { sections, .. } => {
            let mut secs = Vec::new();
            for sec in sections {
                let ss = st
                    .sketch(sec.sketch)
                    .ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sec.sketch)))?;
                let regions = solvecraft_sketch::merge_regions(&select_profiles(ss, &sec.profiles)?);
                let [r] = &regions[..] else { return Err(DocError::Invalid("each loft section must be one profile".into())) };
                secs.push((ss.plane, r.outer.clone()));
            }
            Ok(vec![kernel::loft(&secs)?])
        }
        FeatureKind::Sweep { sketch, profiles, path_sketch, path, .. } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            let ps =
                st.sketch(*path_sketch).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            let regions = solvecraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
            let start = regions.first().map(|r| ss.plane.to_world(r.centroid())).unwrap_or_default();
            let segs = path_segments(ps, path, start)?;
            regions.iter().map(|r| kernel::sweep(&ss.plane, r, &segs).map_err(DocError::from)).collect()
        }
        FeatureKind::Pipe { path_sketch, path, diameter, wall, .. } => {
            let ps =
                st.sketch(*path_sketch).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            let r = val(vals, diameter, Kind::Length)? / 2.0;
            if !(r > 1e-6) {
                return Err(DocError::Invalid("pipe diameter must be positive".into()));
            }
            let segs = path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?;
            let first = segs.first().ok_or_else(|| DocError::Invalid("empty path".into()))?;
            let (start, tangent) = match *first {
                kernel::PathSeg::Line { a, b } => (a, b - a),
                kernel::PathSeg::Arc { a, center, axis, .. } => (a, axis.cross(a - center)),
            };
            let t = tangent.normalized().ok_or_else(|| DocError::Invalid("path direction".into()))?;
            let plane = Plane::from_normal(start, t).ok_or_else(|| DocError::Invalid("pipe profile plane".into()))?;
            let holes = match wall {
                Some(w) => {
                    let wt = val(vals, w, Kind::Length)?;
                    if !(wt > 0.0 && wt < r) {
                        return Err(DocError::Invalid("the wall must be thinner than the radius".into()));
                    }
                    vec![solvecraft_geom::Loop2::circle(Vec2::ZERO, r - wt).reversed()]
                }
                None => Vec::new(),
            };
            let region = Region2 { outer: solvecraft_geom::Loop2::circle(Vec2::ZERO, r), holes };
            Ok(vec![kernel::sweep(&plane, &region, &segs)?])
        }
        FeatureKind::Box { corner, length, width, height, .. } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            Ok(vec![kernel::box_solid(*corner, *corner + s)?])
        }
        FeatureKind::Cylinder { base, axis, radius, height, .. } => {
            Ok(vec![kernel::cylinder(*base, *axis, val(vals, radius, Kind::Length)?, val(vals, height, Kind::Length)?)?])
        }
        FeatureKind::Sphere { center, radius, .. } => Ok(vec![kernel::sphere(*center, val(vals, radius, Kind::Length)?)?]),
        FeatureKind::Torus { center, major, minor, .. } => {
            Ok(vec![kernel::torus(*center, val(vals, major, Kind::Length)?, val(vals, minor, Kind::Length)?)?])
        }
        FeatureKind::Hole { diameter, depth, hole, .. } => {
            let (spots, dir) = hole_spots(st, f)?;
            if spots.is_empty() || spots.len() > 10_000 {
                return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
            }
            let mut tools = Vec::new();
            for at in spots {
                tools.extend(hole_tools(vals, st, &at, dir, diameter, depth, hole)?);
            }
            Ok(tools)
        }
        _ => Err(DocError::Invalid(format!("{} cannot be patterned or mirrored yet", f.name))),
    }
}

/// A chain of sketch curves as 3D path segments, starting at the end nearest `start`.
fn path_segments(ps: &SolvedSketch, ids: &[String], start: Vec3) -> Result<Vec<kernel::PathSeg>> {
    if ids.is_empty() || ids.len() > 1000 {
        return Err(DocError::Invalid("the path needs 1…1000 curves".into()));
    }
    let mut segs: Vec<solvecraft_geom::Seg2> = Vec::new();
    for id in ids {
        let ci = ps.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("path curve `{id}`")))?;
        segs.extend(ps.sketch.segs(ci));
    }
    // Orient the chain: the first segment starts at the end nearest `start`, each next one
    // starts where the previous ended.
    let w = |p: Vec2| ps.plane.to_world(p);
    if let (Some(f), true) = (segs.first().copied(), !segs.is_empty())
        && w(f.end()).dist(start) < w(f.start()).dist(start)
        && (segs.len() == 1 || segs.get(1).is_some_and(|n| n.start().dist(f.start()) < 1e-6 || n.end().dist(f.start()) < 1e-6))
    {
        segs[0] = f.reversed();
    }
    // No start given (a pipe): the first curve runs toward the second.
    if !start.is_finite()
        && let (Some(f), Some(n)) = (segs.first().copied(), segs.get(1).copied())
        && (n.start().dist(f.start()) < 1e-6 || n.end().dist(f.start()) < 1e-6)
    {
        segs[0] = f.reversed();
    }
    for i in 1..segs.len() {
        let prev_end = segs[i - 1].end();
        if segs[i].end().dist(prev_end) < segs[i].start().dist(prev_end) {
            segs[i] = segs[i].reversed();
        }
        if segs[i].start().dist(prev_end) > 1e-5 {
            return Err(DocError::Invalid("the path curves are not connected end to end".into()));
        }
    }
    let n = ps.plane.normal();
    Ok(segs
        .iter()
        .flat_map(|s| match *s {
            solvecraft_geom::Seg2::Line { a, b } => vec![kernel::PathSeg::Line { a: w(a), b: w(b) }],
            solvecraft_geom::Seg2::Arc { center, sweep, .. } => {
                vec![kernel::PathSeg::Arc { a: w(s.start()), center: w(center), axis: n * sweep.signum(), angle: sweep.abs() }]
            }
            // Free-form path curves are followed as polylines.
            solvecraft_geom::Seg2::Cubic { .. } | solvecraft_geom::Seg2::Conic { .. } => {
                s.polyline(1e-2).windows(2).map(|q| kernel::PathSeg::Line { a: w(q[0]), b: w(q[1]) }).collect()
            }
        })
        .collect())
}

/// Operation and targets of a feature that makes tools.
fn feature_op(f: &Feature) -> (Operation, Vec<String>) {
    match &f.kind {
        FeatureKind::Extrude { operation, targets, .. } | FeatureKind::Revolve { operation, targets, .. } => (*operation, targets.clone()),
        FeatureKind::Box { operation, .. }
        | FeatureKind::Cylinder { operation, .. }
        | FeatureKind::Sphere { operation, .. }
        | FeatureKind::Torus { operation, .. } => (*operation, Vec::new()),
        FeatureKind::Hole { .. } => (Operation::Cut, Vec::new()),
        FeatureKind::Pipe { operation, targets, .. } => (*operation, targets.clone()),
        FeatureKind::Loft { operation, targets, .. } | FeatureKind::Sweep { operation, targets, .. } => (*operation, targets.clone()),
        FeatureKind::Coil { operation, targets, .. } => (*operation, targets.clone()),
        _ => (Operation::NewBody, Vec::new()),
    }
}

type Mat = [[f64; 4]; 4];

fn translation(t: Vec3) -> Mat {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [t.x, t.y, t.z, 1.0]]
}

/// Rotation by `ang` about the line through `o` with direction `a` (column-major).
fn rotation(o: Vec3, a: Vec3, ang: f64) -> Mat {
    let a = a.normalized().unwrap_or(Vec3::Z);
    let (c, s) = (ang.cos(), ang.sin());
    let t = 1.0 - c;
    let r = [
        [t * a.x * a.x + c, t * a.x * a.y + s * a.z, t * a.x * a.z - s * a.y],
        [t * a.x * a.y - s * a.z, t * a.y * a.y + c, t * a.y * a.z + s * a.x],
        [t * a.x * a.z + s * a.y, t * a.y * a.z - s * a.x, t * a.z * a.z + c],
    ];
    // p' = R (p - o) + o
    let ro = Vec3::new(
        r[0][0] * o.x + r[1][0] * o.y + r[2][0] * o.z,
        r[0][1] * o.x + r[1][1] * o.y + r[2][1] * o.z,
        r[0][2] * o.x + r[1][2] * o.y + r[2][2] * o.z,
    );
    let tr = o - ro;
    [[r[0][0], r[0][1], r[0][2], 0.0], [r[1][0], r[1][1], r[1][2], 0.0], [r[2][0], r[2][1], r[2][2], 0.0], [tr.x, tr.y, tr.z, 1.0]]
}

/// Reflection in a plane (column-major).
fn mirror_matrix(pl: &Plane) -> Mat {
    let n = pl.normal();
    let d = pl.origin.dot(n);
    let m = |i: usize, j: usize| {
        let (ni, nj) = ([n.x, n.y, n.z][i], [n.x, n.y, n.z][j]);
        (if i == j { 1.0 } else { 0.0 }) - 2.0 * ni * nj
    };
    // p' = p - 2 (p·n - d) n
    [
        [m(0, 0), m(0, 1), m(0, 2), 0.0],
        [m(1, 0), m(1, 1), m(1, 2), 0.0],
        [m(2, 0), m(2, 1), m(2, 2), 0.0],
        [2.0 * d * n.x, 2.0 * d * n.y, 2.0 * d * n.z, 1.0],
    ]
}

const MAX_INSTANCES: usize = 1000;

/// The placements (column-major matrices) of a pattern's copies, the original excluded.
pub fn pattern_matrices(doc: &Document, st: &ModelState, p: &crate::PatternKind) -> Result<Vec<[[f64; 4]; 4]>> {
    let (vals, _) = doc.param_values();
    pattern_transforms(&vals, st, p)
}

fn pattern_transforms(vals: &BTreeMap<String, Value>, st: &ModelState, p: &crate::PatternKind) -> Result<Vec<Mat>> {
    let count = |e: &str| -> Result<usize> {
        let n = val(vals, e, Kind::Unitless)?.round();
        if !(1.0..=MAX_INSTANCES as f64).contains(&n) {
            return Err(DocError::Invalid(format!("pattern count must be 1…{MAX_INSTANCES}")));
        }
        Ok(n as usize)
    };
    let mut out = Vec::new();
    match p {
        crate::PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2 } => {
            let (n1, s1) = (count(count1)?, val(vals, spacing1, Kind::Length)?);
            let d1 = dir1.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?;
            let (n2, s2, d2) = match (dir2, count2, spacing2) {
                (Some(d), Some(c), Some(sp)) => {
                    (count(c)?, val(vals, sp, Kind::Length)?, d.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?)
                }
                _ => (1, 0.0, Vec3::Y),
            };
            if n1 * n2 > MAX_INSTANCES {
                return Err(DocError::Invalid("too many pattern instances".into()));
            }
            for i in 0..n1 {
                for j in 0..n2 {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    out.push(translation(d1 * (s1 * i as f64) + d2 * (s2 * j as f64)));
                }
            }
        }
        crate::PatternKind::Path { path_sketch, path, count: c, spacing, extent, flip, orient } => {
            let n = count(c)?;
            let d = val(vals, spacing, Kind::Length)?;
            let ps = st.sketch(*path_sketch).ok_or_else(|| DocError::Unknown(format!("path sketch {path_sketch}")))?;
            let mut segs = path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?;
            if *flip {
                segs = segs.iter().rev().map(reverse_seg).collect();
            }
            let total: f64 = segs.iter().map(seg_len).sum();
            let step = if *extent { if n > 1 { d / (n - 1) as f64 } else { 0.0 } } else { d };
            if (step * (n - 1) as f64).abs() > total * (1.0 + 1e-5) + 1e-6 {
                return Err(DocError::Invalid(format!("the instances run past the end of the path ({total:.3} mm long)")));
            }
            let normal = ps.plane.normal();
            let (p0, t0) = path_at(&segs, 0.0);
            for i in 1..n {
                let (pk, tk) = path_at(&segs, step * i as f64);
                let mut m = if *orient {
                    // Signed turn of the tangent about the sketch normal.
                    let ang = t0.cross(tk).dot(normal).atan2(t0.dot(tk));
                    rotation(p0, normal, ang)
                } else {
                    translation(Vec3::ZERO)
                };
                let dv = pk - p0;
                m[3][0] += dv.x;
                m[3][1] += dv.y;
                m[3][2] += dv.z;
                out.push(m);
            }
        }
        crate::PatternKind::Circular { origin, axis, count: c, angle } => {
            let n = count(c)?;
            let total = val(vals, angle, Kind::Angle)?;
            let full = (total.abs() - std::f64::consts::TAU).abs() < 1e-9;
            let step = if full {
                total / n as f64
            } else if n > 1 {
                total / (n - 1) as f64
            } else {
                0.0
            };
            for i in 1..n {
                out.push(rotation(*origin, *axis, step * i as f64));
            }
        }
    }
    Ok(out)
}

fn seg_len(s: &kernel::PathSeg) -> f64 {
    match *s {
        kernel::PathSeg::Line { a, b } => (b - a).len(),
        kernel::PathSeg::Arc { a, center, angle, .. } => (a - center).len() * angle.abs(),
    }
}

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rotate_vec(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Point and unit tangent `t` mm along a segment.
fn seg_at(s: &kernel::PathSeg, t: f64) -> (Vec3, Vec3) {
    match *s {
        kernel::PathSeg::Line { a, b } => {
            let d = (b - a).normalized().unwrap_or(Vec3::X);
            (a + d * t, d)
        }
        kernel::PathSeg::Arc { a, center, axis, angle } => {
            let k = axis.normalized().unwrap_or(Vec3::Z);
            let r = (a - center).len().max(1e-12);
            // Sweeps turn right-handed about `axis` (angles are positive).
            let s = if angle < 0.0 { -1.0 } else { 1.0 };
            let v = rotate_vec(a - center, k, s * t / r);
            (center + v, k.cross(v).normalized().unwrap_or(Vec3::X) * s)
        }
    }
}

fn reverse_seg(s: &kernel::PathSeg) -> kernel::PathSeg {
    match *s {
        kernel::PathSeg::Line { a, b } => kernel::PathSeg::Line { a: b, b: a },
        kernel::PathSeg::Arc { a, center, axis, angle } => {
            let k = axis.normalized().unwrap_or(Vec3::Z);
            let end = center + rotate_vec(a - center, k, angle);
            kernel::PathSeg::Arc { a: end, center, axis: k * -1.0, angle }
        }
    }
}

/// Point and tangent at arc length `d` along a chain (clamped to its ends).
fn path_at(segs: &[kernel::PathSeg], d: f64) -> (Vec3, Vec3) {
    let mut left = d.max(0.0);
    for (i, s) in segs.iter().enumerate() {
        let l = seg_len(s);
        if left <= l + 1e-9 || i + 1 == segs.len() {
            return seg_at(s, left.min(l));
        }
        left -= l;
    }
    (Vec3::ZERO, Vec3::X)
}

/// Apply transformed copies of the tools of `features` (patterns, mirrors).
fn replay(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, features: &[String], mats: &[Mat]) -> Result<()> {
    if features.is_empty() {
        return Err(DocError::Invalid("no features selected".into()));
    }
    for name in features {
        // A sketch and its extrude often share a name; the feature that makes geometry wins.
        let src = doc
            .features
            .iter()
            .find(|x| &x.name == name && !matches!(x.kind, FeatureKind::Sketch { .. } | FeatureKind::ConstructionPlane { .. }))
            .or_else(|| doc.find_feature(name))
            .ok_or_else(|| DocError::Unknown(format!("feature `{name}`")))?;
        // A fillet or chamfer repeats at the copied edges.
        if let FeatureKind::Fillet { edges, radius, .. } | FeatureKind::Chamfer { edges, distance: radius, .. } = &src.kind {
            let r = val(vals, radius, Kind::Length)?;
            for m in mats {
                let moved: Vec<Vec3> = edges.iter().map(|p| crate::apply_point(m, *p)).collect();
                let ti = blend_target(st, &None, &moved)?;
                let Some(mb) = st.bodies.get(ti).cloned() else { continue };
                let nb = if matches!(src.kind, FeatureKind::Fillet { .. }) {
                    kernel::fillet(&mb.body, &moved, r)?
                } else {
                    kernel::chamfer(&mb.body, &moved, r)?
                };
                if let Some(slot) = st.bodies.get_mut(ti) {
                    *slot = ModelBody::new(mb.name, nb, mb.feature);
                }
            }
            continue;
        }
        let tools = feature_tools(vals, src, st)?;
        let (op, targets) = feature_op(src);
        let mut copies = Vec::new();
        for m in mats {
            for t in &tools {
                copies.push(kernel::transform_matrix(t, *m)?);
            }
        }
        // New bodies from a pattern join their source when they touch it (as one feature would).
        apply_op(st, f, copies, op, &targets)?;
    }
    Ok(())
}

#[path = "eval_more.rs"]
mod more;

fn eval_feature(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    match &f.kind {
        FeatureKind::SheetBase { .. }
        | FeatureKind::SheetContour { .. }
        | FeatureKind::SheetFlange { .. }
        | FeatureKind::SheetHem { .. }
        | FeatureKind::SheetUnfold { .. }
        | FeatureKind::SheetConvert { .. } => more::sheet_eval(doc, vals, f, st),
        FeatureKind::Emboss { .. }
        | FeatureKind::Rib { .. }
        | FeatureKind::ReplaceFace { .. }
        | FeatureKind::Align { .. }
        | FeatureKind::Remove { .. } => more::eval(doc, vals, f, st),
        FeatureKind::Sketch { plane, sketch } => {
            let plane = doc.resolve_plane(vals, plane, 0)?;
            let mut sk = sketch.clone();
            let mut warns = crate::project::refresh_links(doc, vals, st, &plane, &mut sk);
            doc.apply_dimension_values(vals, &mut sk)?;
            let report = solve(&mut sk);
            if !report.ok() {
                warns.insert(0, format!("the sketch constraints conflict ({})", report.failing.join(", ")));
            }
            if !warns.is_empty() {
                *warning = Some(warns.join("; "));
            }
            let profiles = find_profiles(&sk);
            st.sketches.retain(|s| s.feature != f.id);
            st.sketches.push(SolvedSketch { feature: f.id, name: f.name.clone(), plane, sketch: sk, report, profiles });
            Ok(())
        }
        FeatureKind::Extrude { operation, targets, .. }
        | FeatureKind::Pipe { operation, targets, .. }
        | FeatureKind::Revolve { operation, targets, .. }
        | FeatureKind::Loft { operation, targets, .. }
        | FeatureKind::Coil { operation, targets, .. }
        | FeatureKind::Sweep { operation, targets, .. } => {
            let tools = feature_tools(vals, f, st)?;
            // Cuts through sheet metal become cut-outs of its flat pattern.
            if *operation == Operation::Cut && matches!(f.kind, FeatureKind::Extrude { .. }) && !st.sheets.is_empty() {
                let handled = more::sheet_cut(f, st, &tools, targets)?;
                if !handled.is_empty() {
                    let rest: Vec<String> = st
                        .bodies
                        .iter()
                        .map(|b| b.name.clone())
                        .filter(|n| !handled.contains(n) && (targets.is_empty() || targets.contains(n)))
                        .collect();
                    if rest.is_empty() {
                        return Ok(());
                    }
                    return apply_op(st, f, tools, *operation, &rest);
                }
            }
            apply_op(st, f, tools, *operation, targets)
        }
        FeatureKind::Hole { thread, diameter, .. } => {
            let (spots, dir) = hole_spots(st, f)?;
            let tools = feature_tools(vals, f, st)?;
            apply_op(st, f, tools, Operation::Cut, &[])?;
            if let Some(d) = thread {
                // A point on each hole's wall, a little below the entry.
                let r = val(vals, diameter, Kind::Length)? / 2.0;
                for p in spots {
                    let wall = p + dir.any_perp() * r + dir * (r * 0.5);
                    let t = thread_on(st, f.id, wall, Some(d), None)?;
                    st.threads.push(t);
                }
            }
            Ok(())
        }
        FeatureKind::Thread { face, designation, length } => {
            let l = match length {
                Some(e) => Some(val(vals, e, Kind::Length)?),
                None => None,
            };
            let t = thread_on(st, f.id, *face, designation.as_deref(), l)?;
            st.threads.push(t);
            Ok(())
        }
        FeatureKind::Shell { faces, thickness, body } => {
            let t = val(vals, thickness, Kind::Length)?;
            let i = body_at(st, body, faces)?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = kernel::shell(&mb.body, faces, t)?;
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Draft { faces, angle, neutral, pull, body } => {
            let a = val(vals, angle, Kind::Angle)?;
            let pl = doc.resolve_plane(vals, neutral, 0)?;
            let i = body_at(st, body, faces)?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = kernel::draft(&mb.body, faces, &pl, *pull, a)?;
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::ConstructionPlane { plane } => {
            doc.resolve_plane(vals, plane, 0)?;
            Ok(())
        }
        FeatureKind::Split { body, plane } => {
            let pl = doc.resolve_plane(vals, plane, 0)?;
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Unknown(format!("body `{body}`"))) };
            let parts = kernel::split_by_plane(&mb.body, &pl)?;
            if parts.len() < 2 {
                return Err(DocError::Invalid("the plane does not split the body".into()));
            }
            let mut it = parts.into_iter();
            if let (Some(first), Some(slot)) = (it.next(), st.bodies.get_mut(i)) {
                *slot = ModelBody::new(mb.name.clone(), first, mb.feature);
            }
            for (k, b) in it.enumerate() {
                let mut name = format!("{} ({})", mb.name, k + 1);
                while st.body(&name).is_some() {
                    name.push('\'');
                }
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Pattern { features, pattern, bodies } => {
            let mats = pattern_transforms(vals, st, pattern)?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &mats);
            }
            // Bodies: each copy is a new body named after its source.
            for n in bodies {
                let mb = st.body(n).cloned().ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                for m in &mats {
                    let copy = kernel::transform_matrix(&mb.body, *m)?;
                    let name = unique_body_name(st, &mb.name);
                    st.bodies.push(ModelBody::new(name, copy, f.id));
                }
            }
            Ok(())
        }
        FeatureKind::Mirror { features, plane, bodies, combine } => {
            let pl = doc.resolve_plane(vals, plane, 0)?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &[mirror_matrix(&pl)]);
            }
            let m = mirror_matrix(&pl);
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { continue };
                let mirrored = kernel::transform_matrix(&mb.body, m)?;
                if *combine {
                    let joined = kernel::boolean(&mb.body, &mirrored, BoolOp::Union)?
                        .ok_or_else(|| DocError::Invalid("the mirror join produced nothing".into()))?;
                    for (k, p) in joined.lumps()?.into_iter().enumerate() {
                        if k == 0 {
                            if let Some(slot) = st.bodies.get_mut(i) {
                                *slot = ModelBody::new(mb.name.clone(), p, mb.feature);
                            }
                        } else {
                            let name = unique_body_name(st, &mb.name);
                            st.bodies.push(ModelBody::new(name, p, f.id));
                        }
                    }
                } else {
                    let name = unique_body_name(st, &mb.name);
                    st.bodies.push(ModelBody::new(name, mirrored, f.id));
                }
            }
            Ok(())
        }
        FeatureKind::Fillet { edges, radius, body } | FeatureKind::Chamfer { edges, distance: radius, body } => {
            let r = val(vals, radius, Kind::Length)?;
            let ti = blend_target(st, body, edges)?;
            let Some(mb) = st.bodies.get(ti) else { return Err(DocError::Invalid("body".into())) };
            let edges = resolve_edges(&mb.body, edges, warning)?;
            let nb = if matches!(f.kind, FeatureKind::Fillet { .. }) {
                kernel::fillet(&mb.body, &edges, r)?
            } else {
                kernel::chamfer(&mb.body, &edges, r)?
            };
            let (name, feat) = (mb.name.clone(), mb.feature);
            if let Some(slot) = st.bodies.get_mut(ti) {
                *slot = ModelBody::new(name, nb, feat);
            }
            Ok(())
        }
        FeatureKind::Box { corner, length, width, height, operation } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            let b = kernel::box_solid(*corner, *corner + s)?;
            apply_op(st, f, vec![b], *operation, &[])
        }
        FeatureKind::Cylinder { base, axis, radius, height, operation } => {
            let b = kernel::cylinder(*base, *axis, val(vals, radius, Kind::Length)?, val(vals, height, Kind::Length)?)?;
            apply_op(st, f, vec![b], *operation, &[])
        }
        FeatureKind::Sphere { center, radius, operation } => {
            let b = kernel::sphere(*center, val(vals, radius, Kind::Length)?)?;
            apply_op(st, f, vec![b], *operation, &[])
        }
        FeatureKind::Torus { center, major, minor, operation } => {
            let b = kernel::torus(*center, val(vals, major, Kind::Length)?, val(vals, minor, Kind::Length)?)?;
            apply_op(st, f, vec![b], *operation, &[])
        }
        FeatureKind::Combine { target, tools, operation, keep_tools } => {
            if tools.iter().any(|t| t == target) {
                return Err(DocError::Invalid("a body cannot be combined with itself".into()));
            }
            let mut tb = Vec::new();
            for t in tools {
                tb.push(st.body(t).ok_or_else(|| DocError::Unknown(format!("body `{t}`")))?.body.clone());
            }
            if st.body(target).is_none() {
                return Err(DocError::Unknown(format!("body `{target}`")));
            }
            let op = if *operation == Operation::NewBody { Operation::Join } else { *operation };
            apply_op(st, f, tb, op, std::slice::from_ref(target))?;
            if !keep_tools {
                st.bodies.retain(|b| !tools.contains(&b.name));
            }
            Ok(())
        }
        FeatureKind::Scale { bodies, origin, factor, factors } => {
            let k = match factors {
                Some(fs) => [val(vals, &fs[0], Kind::Unitless)?, val(vals, &fs[1], Kind::Unitless)?, val(vals, &fs[2], Kind::Unitless)?],
                None => {
                    let x = val(vals, factor, Kind::Unitless)?;
                    [x, x, x]
                }
            };
            if k.iter().any(|x| !(x.is_finite() && *x > 1e-9 && *x < 1e6)) {
                return Err(DocError::Invalid("scale factors must be positive".into()));
            }
            let o = *origin;
            // Column-major: scale about the origin.
            let m = [
                [k[0], 0.0, 0.0, 0.0],
                [0.0, k[1], 0.0, 0.0],
                [0.0, 0.0, k[2], 0.0],
                [o.x * (1.0 - k[0]), o.y * (1.0 - k[1]), o.z * (1.0 - k[2]), 1.0],
            ];
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                if let Some(mb) = st.bodies.get_mut(i) {
                    let nb = kernel::transform_matrix(&mb.body, m)?;
                    *mb = ModelBody::new(mb.name.clone(), nb, mb.feature);
                }
            }
            Ok(())
        }
        FeatureKind::OffsetFace { faces, distance, body } => {
            let d = val(vals, distance, Kind::Length)?;
            let i = body_at(st, body, faces)?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = kernel::offset_faces(&mb.body, faces, d)?;
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::BoundingSolid { bodies, margin } => {
            let m = val(vals, margin, Kind::Length)?;
            let mut bb = solvecraft_geom::Aabb3::EMPTY;
            for b in st.bodies.iter().filter(|b| bodies.is_empty() || bodies.contains(&b.name)) {
                bb = bb.union(&b.mesh().bounds());
            }
            if bb.is_empty() {
                return Err(DocError::Invalid("no bodies to bound".into()));
            }
            let pad = Vec3::new(m, m, m);
            let b = kernel::box_solid(bb.min - pad, bb.max + pad)?;
            let name = unique_body_name(st, f.body_names.first().map(String::as_str).unwrap_or("Bounding"));
            st.bodies.push(ModelBody::new(name, b, f.id));
            Ok(())
        }
        FeatureKind::Move { bodies, translate, rotate_axis, angle } => {
            let t =
                Vec3::new(val(vals, &translate[0], Kind::Length)?, val(vals, &translate[1], Kind::Length)?, val(vals, &translate[2], Kind::Length)?);
            let ang = match angle {
                Some(a) => val(vals, a, Kind::Angle)?,
                None => 0.0,
            };
            let axis = rotate_axis.unwrap_or(Vec3::Z);
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                if let Some(mb) = st.bodies.get_mut(i) {
                    let nb = kernel::transform(&mb.body, t, Vec3::ZERO, axis, ang)?;
                    *mb = ModelBody::new(mb.name.clone(), nb, mb.feature);
                }
            }
            Ok(())
        }
        FeatureKind::MeshImport { meshes, .. } => {
            for (name, b) in mesh_bodies(meshes)? {
                let name = unique_body_name(st, &name);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Import { step, .. } => {
            let imp = kernel::step_import_shared(step)?;
            for (k, b) in imp.bodies.iter().enumerate() {
                let base = f.body_names.get(k).filter(|n| !n.trim().is_empty()).unwrap_or(&b.name);
                let name = unique_body_name(st, base);
                st.bodies.push(ModelBody::new(name, b.body.clone(), f.id));
            }
            if !imp.warnings.is_empty() {
                let n = imp.warnings.len();
                let shown: Vec<&str> = imp.warnings.iter().take(3).map(String::as_str).collect();
                let more = if n > 3 { format!(" (+{} more)", n - 3) } else { String::new() };
                *warning = Some(format!("{}{more}", shown.join("; ")));
            }
            Ok(())
        }
    }
}

/// Mesh bodies of a MeshImport feature.
fn mesh_bodies(meshes: &[crate::MeshData]) -> Result<Vec<(String, Body)>> {
    let mut out = Vec::with_capacity(meshes.len());
    for m in meshes {
        let positions: Vec<Vec3> = m.positions.as_chunks::<3>().0.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect();
        let triangles: Vec<[u32; 3]> = m.triangles.as_chunks::<3>().0.to_vec();
        let b = kernel::mesh_body(&positions, &triangles).map_err(|e| DocError::Invalid(format!("mesh `{}`: {e}", m.name)))?;
        out.push((m.name.clone(), b.with_color(m.color)));
    }
    Ok(out)
}

/// `base`, or `base (2)`, `base (3)`… if a body already has that name.
fn unique_body_name(st: &ModelState, base: &str) -> String {
    let base = if base.trim().is_empty() { "Body" } else { base.trim() };
    if st.body(base).is_none() {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| st.body(n).is_none()).unwrap_or_else(|| base.to_string())
}

/// The model placed in the world: each component's bodies, sketches and threads through every
/// occurrence of it (bodies of the first placement keep their names; further instances get the
/// occurrence path in brackets). Without components this is the model itself.
pub fn world_state(doc: &Document, st: &ModelState) -> ModelState {
    if doc.occurrences.is_empty() {
        return st.clone();
    }
    let comp_of_feature = |id: u64| doc.feature(id).map(|f| f.component).unwrap_or(0);
    let first_path = |comp: u64| -> Vec<u64> {
        let mut path = Vec::new();
        let mut c = comp;
        for _ in 0..1000 {
            if c == 0 {
                break;
            }
            let Some(o) = doc.occurrence_of(c) else { break };
            path.insert(0, o.id);
            c = o.parent;
        }
        path
    };
    let mut out = ModelState { body_counter: st.body_counter, ..Default::default() };
    for (comp, path, m) in doc.placements() {
        let primary = path == first_path(comp);
        let label = || -> String {
            path.iter().filter_map(|id| doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.name.as_str()).collect::<Vec<_>>().join("/")
        };
        let ident = crate::is_identity(&m);
        for b in st.bodies.iter().filter(|b| doc.body_component(&b.name, b.feature) == comp) {
            let name = if primary { b.name.clone() } else { format!("{} ({})", b.name, label()) };
            let body = if ident { b.body.clone() } else { kernel::transform_matrix(&b.body, m).unwrap_or_else(|_| b.body.clone()) };
            out.bodies.push(ModelBody::new(name, body, b.feature));
        }
        if primary {
            for s in st.sketches.iter().filter(|s| comp_of_feature(s.feature) == comp) {
                let mut s2 = s.clone();
                if !ident {
                    let p = &s.plane;
                    if let Some(np) = Plane::new(crate::apply_point(&m, p.origin), crate::apply_vector(&m, p.x), crate::apply_vector(&m, p.y)) {
                        s2.plane = np;
                    }
                }
                out.sketches.push(s2);
            }
            for t in st.threads.iter().filter(|t| comp_of_feature(t.feature) == comp) {
                let mut t2 = t.clone();
                t2.axis_point = crate::apply_point(&m, t.axis_point);
                t2.axis = crate::apply_vector(&m, t.axis);
                out.threads.push(t2);
            }
        }
    }
    out
}
