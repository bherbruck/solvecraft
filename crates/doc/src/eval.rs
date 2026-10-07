//! Timeline evaluation.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use solvecraft_geom::{Mesh, Plane, Region2, Vec2, Vec3};
use solvecraft_kernel::{self as kernel, Body, BoolOp};
use solvecraft_sketch::{CurveKind, Profile, SolveReport, find_profiles, solve};

use crate::expr::{self, Kind, Value};
use crate::{AxisRef, DocError, Document, Feature, FeatureKind, Operation, ProfileSel, Result};

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

/// Model after some prefix of the timeline.
#[derive(Clone, Debug, Default)]
pub struct ModelState {
    pub bodies: Vec<ModelBody>,
    pub sketches: Vec<SolvedSketch>,
    pub body_counter: usize,
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

fn fingerprint(prev: u64, f: &Feature, vals: &BTreeMap<String, Value>, rolled_back: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prev.hash(&mut h);
    rolled_back.hash(&mut h);
    serde_json::to_string(f).unwrap_or_default().hash(&mut h);
    let mut names: Vec<String> = f.kind.expressions().iter().flat_map(|e| expr::references(e)).collect();
    if let FeatureKind::Sketch { sketch, .. } = &f.kind {
        names.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
    }
    names.sort();
    names.dedup();
    for n in names {
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

fn now() -> std::time::Instant {
    std::time::Instant::now()
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
            let fp = fingerprint(prev_fp, f, &vals, rolled_back);
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
            let r = kernel::guard(&f.name, || {
                eval_feature(doc, &vals, f, &mut next, &mut warning).map_err(|e| kernel::KernelError::Failed(e.to_string()))
            });
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
        ProfileSel::All => ps.iter().map(|p| p.region.clone()).collect(),
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
        }
        Operation::NewBody => {}
    }
    Ok(())
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
    best.map(|(i, _)| i).ok_or_else(|| DocError::Invalid("there is no body to modify".into()))
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
    }
}

fn eval_feature(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    match &f.kind {
        FeatureKind::Sketch { plane, sketch } => {
            let plane = doc.resolve_plane(vals, plane, 0)?;
            let mut sk = sketch.clone();
            doc.apply_dimension_values(vals, &mut sk)?;
            let report = solve(&mut sk);
            if !report.ok() {
                *warning = Some(format!("the sketch constraints conflict ({})", report.failing.join(", ")));
            }
            let profiles = find_profiles(&sk);
            st.sketches.retain(|s| s.feature != f.id);
            st.sketches.push(SolvedSketch { feature: f.id, name: f.name.clone(), plane, sketch: sk, report, profiles });
            Ok(())
        }
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?.clone();
            let regions = select_profiles(&ss, profiles)?;
            let d = val(vals, &extent.distance, Kind::Length)?;
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
                tools.extend(kernel::extrude(&ss.plane, std::slice::from_ref(r), l2, h2)?);
            }
            apply_op(st, f, tools, *operation, targets)
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, operation, targets } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?.clone();
            let regions = select_profiles(&ss, profiles)?;
            let (o, d) = revolve_axis(&ss, axis)?;
            let ang = val(vals, angle, Kind::Angle)?;
            let tools = kernel::revolve(&ss.plane, &regions, o, d, ang)?;
            apply_op(st, f, tools, *operation, targets)
        }
        FeatureKind::Fillet { edges, radius, body } | FeatureKind::Chamfer { edges, distance: radius, body } => {
            let r = val(vals, radius, Kind::Length)?;
            let ti = blend_target(st, body, edges)?;
            let Some(mb) = st.bodies.get(ti) else { return Err(DocError::Invalid("body".into())) };
            let nb =
                if matches!(f.kind, FeatureKind::Fillet { .. }) { kernel::fillet(&mb.body, edges, r)? } else { kernel::chamfer(&mb.body, edges, r)? };
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
    }
}
