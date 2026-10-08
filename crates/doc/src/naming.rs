//! Persistent names of faces and edges (plan/adr/0002-persistent-naming.md).
//!
//! Every face of an evaluated body is named after how it was made: a face that lies on a face
//! of the feature's input keeps that face's name; a face new to the feature is named by its role
//! (`F<feature>:side:<sketch curve>`, `F<feature>:start`/`end`, `F<feature>:blend:<k>`, …).
//! Pieces of a split face get `#1`, `#2`, … along the longest axis. Edges are named by the faces
//! they separate. Names are worked out lazily, on first request, from the body, the feature
//! that made it and the model before that feature.

use std::sync::{Arc, OnceLock};

use solvecraft_geom::{Mesh, Seg2, Vec2, Vec3};

use crate::{Feature, FeatureKind, ModelBody, ModelState};

/// How a body came to be: the feature and the model just before it.
#[derive(Debug)]
pub struct NamingOrigin {
    pub feature: Feature,
    pub before: Arc<ModelState>,
    pub depth: usize,
    /// Instance transforms of a pattern or mirror (its copies are named after their sources).
    pub mats: Vec<crate::Mat>,
}

/// The lazily computed names of a body, shared by its clones.
#[derive(Clone, Debug, Default)]
pub struct NameCell {
    pub origin: Option<Arc<NamingOrigin>>,
    names: Arc<OnceLock<Arc<Vec<String>>>>,
}

impl NameCell {
    pub fn new(origin: Option<Arc<NamingOrigin>>) -> Self {
        NameCell { origin, names: Arc::new(OnceLock::new()) }
    }
}

/// Deeper chains than this are named by position only (bounded work and stack).
const MAX_DEPTH: usize = 400;

/// Up to this many sample points per face.
const SAMPLES: usize = 9;

/// Unit normals of each face's triangles.
fn normals(m: &Mesh, nf: usize) -> Vec<Vec<Vec3>> {
    let mut per: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(v), Some([a, b, c])) = (per.get_mut(*f as usize), m.tri(t))
            && let Some(n) = (b - a).cross(c - a).normalized()
        {
            v.push(n);
        }
    }
    per
}

/// Triangle centroids spread over each face: face → points.
fn samples(m: &Mesh, nf: usize) -> Vec<Vec<Vec3>> {
    let mut per: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(v), Some([a, b, c])) = (per.get_mut(*f as usize), m.tri(t)) {
            v.push((a + b + c) * (1.0 / 3.0));
        }
    }
    per.into_iter()
        .map(|v| {
            if v.len() <= SAMPLES {
                return v;
            }
            let step = v.len() as f64 / SAMPLES as f64;
            (0..SAMPLES).filter_map(|i| v.get((i as f64 * step) as usize).copied()).collect()
        })
        .collect()
}

fn closest(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

/// A body's faces as triangle lists with bounding boxes, for "does this point lie on face f".
struct FaceIndex {
    faces: Vec<(Vec3, Vec3, Vec<[Vec3; 3]>)>,
}

impl FaceIndex {
    fn new(m: &Mesh, nf: usize) -> FaceIndex {
        let mut faces: Vec<(Vec3, Vec3, Vec<[Vec3; 3]>)> =
            vec![(Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN), Vec::new()); nf];
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            if let (Some(slot), Some(tri)) = (faces.get_mut(*f as usize), m.tri(t)) {
                for p in tri {
                    slot.0 = Vec3::new(slot.0.x.min(p.x), slot.0.y.min(p.y), slot.0.z.min(p.z));
                    slot.1 = Vec3::new(slot.1.x.max(p.x), slot.1.y.max(p.y), slot.1.z.max(p.z));
                }
                slot.2.push(tri);
            }
        }
        FaceIndex { faces }
    }

    /// The face a point lies on (within `tol`).
    fn face_at(&self, p: Vec3, tol: f64) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (i, (lo, hi, tris)) in self.faces.iter().enumerate() {
            if p.x < lo.x - tol || p.y < lo.y - tol || p.z < lo.z - tol || p.x > hi.x + tol || p.y > hi.y + tol || p.z > hi.z + tol {
                continue;
            }
            let d = tris.iter().map(|[a, b, c]| closest(p, *a, *b, *c).dist(p)).fold(f64::MAX, f64::min);
            if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}

fn face_count(b: &ModelBody) -> usize {
    b.mesh().tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0)
}

/// Names of a body's faces (by B-rep face index).
pub fn face_names(b: &ModelBody) -> Arc<Vec<String>> {
    b.names
        .names
        .get_or_init(|| {
            let nf = face_count(b);
            let names = match b.names.origin.as_ref() {
                Some(o) if o.depth < MAX_DEPTH => derive(b, o, nf),
                _ => (0..nf).map(|k| format!("{}:face:{k}", b.name)).collect(),
            };
            Arc::new(names)
        })
        .clone()
}

/// Work out a body's face names from the feature that made it and the model before it.
fn derive(b: &ModelBody, o: &NamingOrigin, nf: usize) -> Vec<String> {
    let m = b.mesh();
    let size = b.body.size().max(1e-6);
    let tol = (size * 1e-4).max(1e-6);
    let pts = samples(&m, nf);
    let bb = m.bounds();
    // Inputs that overlap this body.
    let overlaps = |x: &ModelBody| {
        let xb = x.mesh().bounds();
        !(xb.max.x < bb.min.x - tol
            || xb.min.x > bb.max.x + tol
            || xb.max.y < bb.min.y - tol
            || xb.min.y > bb.max.y + tol
            || xb.max.z < bb.min.z - tol
            || xb.min.z > bb.max.z + tol)
    };
    let inputs: Vec<(&ModelBody, FaceIndex, Arc<Vec<String>>)> = o
        .before
        .bodies
        .iter()
        .filter(|x| !x.body.is_mesh() && overlaps(x))
        .take(64)
        .map(|x| {
            let n = face_count(x);
            (x, FaceIndex::new(&x.mesh(), n), face_names(x))
        })
        .collect();
    let fid = o.feature.id;
    let ns = normals(&m, nf);
    let mut names: Vec<Option<String>> = pts.iter().map(|ps| vote(ps, &inputs, tol, None)).collect();
    // A pattern or mirror copy: its faces lie on its source's faces moved by one instance.
    let found = names.iter().filter(|n| n.is_some()).count();
    if !o.mats.is_empty() && found * 2 < nf.max(1) {
        let all: Vec<(&ModelBody, FaceIndex, Arc<Vec<String>>)> = o
            .before
            .bodies
            .iter()
            .filter(|x| !x.body.is_mesh())
            .take(64)
            .map(|x| (x, FaceIndex::new(&x.mesh(), face_count(x)), face_names(x)))
            .collect();
        let mut best: Option<(usize, usize, Vec<Option<String>>)> = None;
        for (k, mat) in o.mats.iter().enumerate() {
            let Some(inv) = crate::mat_inverse(mat) else { continue };
            let got: Vec<Option<String>> = pts.iter().map(|ps| vote(ps, &all, tol, Some(&inv))).collect();
            let n = got.iter().filter(|x| x.is_some()).count();
            if n > found && best.as_ref().is_none_or(|(bn, _, _)| n > *bn) {
                best = Some((n, k, got));
            }
        }
        if let Some((_, k, got)) = best {
            names = got.into_iter().map(|n| n.map(|n| format!("{n}@F{fid}.{}", k + 1))).collect();
        }
    }
    let empty = Vec::new();
    let mut names: Vec<String> = names
        .into_iter()
        .enumerate()
        .map(|(fi, n)| {
            n.unwrap_or_else(|| {
                let (ps, nn) = (pts.get(fi).map(Vec::as_slice).unwrap_or(&[]), ns.get(fi).unwrap_or(&empty));
                // A shell's inner face: the face its wall runs back to.
                if matches!(o.feature.kind, FeatureKind::Shell { .. })
                    && let Some(n) = behind(ps, nn, &inputs)
                {
                    return format!("F{fid}:inner:{}", strip_piece(&n));
                }
                role_name(&o.feature, &o.before, ps, nn, fi, fid)
            })
        })
        .collect();
    number_pieces(&mut names, &pts, '#');
    names
}

/// The input face a face looks back at through the material (along minus its normal): the
/// face a shell wall's inner side offsets.
fn behind(ps: &[Vec3], ns: &[Vec3], inputs: &[(&ModelBody, FaceIndex, Arc<Vec<String>>)]) -> Option<String> {
    let n = ns.first().copied()?;
    let d = n * -1.0;
    let mut votes: Vec<(String, usize)> = Vec::new();
    for p in ps {
        let mut best: Option<(f64, String)> = None;
        for (_, idx, nm) in inputs {
            for (fi, (_, _, tris)) in idx.faces.iter().enumerate() {
                for [a, b, c] in tris {
                    if let Some(t) = ray_hit(*p, d, *a, *b, *c)
                        && t > 1e-9
                        && best.as_ref().is_none_or(|(bt, _)| t < *bt)
                        && let Some(name) = nm.get(fi)
                    {
                        best = Some((t, name.clone()));
                    }
                }
            }
        }
        if let Some((_, n)) = best {
            match votes.iter_mut().find(|(v, _)| *v == n) {
                Some(v) => v.1 += 1,
                None => votes.push((n, 1)),
            }
        }
    }
    votes.sort_by_key(|v| std::cmp::Reverse(v.1));
    votes.first().filter(|(_, k)| *k * 2 >= ps.len().max(1)).map(|(n, _)| n.clone())
}

fn ray_hit(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-14 {
        return None;
    }
    let f = 1.0 / det;
    let s = o - a;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(f * e2.dot(q))
}

/// The input face most of a face's sample points lie on (mapped by `map` first), if it holds
/// at least half of them.
fn vote(ps: &[Vec3], inputs: &[(&ModelBody, FaceIndex, Arc<Vec<String>>)], tol: f64, map: Option<&crate::Mat>) -> Option<String> {
    let mut votes: Vec<(String, usize)> = Vec::new();
    for p in ps {
        let p = match map {
            Some(m) => crate::apply_point(m, *p),
            None => *p,
        };
        for (_, idx, nm) in inputs {
            if let Some(f) = idx.face_at(p, tol)
                && let Some(n) = nm.get(f)
            {
                match votes.iter_mut().find(|(v, _)| v == n) {
                    Some(v) => v.1 += 1,
                    None => votes.push((n.clone(), 1)),
                }
                break;
            }
        }
    }
    votes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    votes.first().filter(|(_, n)| *n * 2 >= ps.len().max(1)).map(|(n, _)| strip_piece(n).to_string())
}

/// A face name without its piece number (`#k`).
pub fn strip_piece(n: &str) -> &str {
    strip_mark(n, '#')
}

/// An edge name without piece numbers: its own (`~k`) and its faces' (`#k`), so pieces of a
/// split edge, or of an edge between split faces, match.
pub fn strip_edge_piece(n: &str) -> String {
    let n = strip_mark(n, '~');
    let mut out = String::with_capacity(n.len());
    let mut it = n.chars().peekable();
    while let Some(c) = it.next() {
        if c == '#' && it.peek().is_some_and(|d| d.is_ascii_digit()) {
            while it.peek().is_some_and(|d| d.is_ascii_digit()) {
                it.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn strip_mark(n: &str, mark: char) -> &str {
    match n.rfind(mark) {
        Some(i) if n[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < n.len() => &n[..i],
        _ => n,
    }
}

/// Faces sharing a name are pieces of one: number them along the longest axis of their spread.
fn number_pieces(names: &mut [String], pts: &[Vec<Vec3>], mark: char) {
    let mut seen: Vec<String> = Vec::new();
    for i in 0..names.len() {
        let n = names[i].clone();
        if seen.contains(&n) {
            continue;
        }
        seen.push(n.clone());
        let same: Vec<usize> = (0..names.len()).filter(|j| names[*j] == n).collect();
        if same.len() < 2 {
            continue;
        }
        let centre = |j: usize| {
            let ps = pts.get(j).cloned().unwrap_or_default();
            ps.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / ps.len().max(1) as f64)
        };
        let cs: Vec<(usize, Vec3)> = same.iter().map(|j| (*j, centre(*j))).collect();
        let (lo, hi) = cs.iter().fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(l, h), (_, c)| {
            (Vec3::new(l.x.min(c.x), l.y.min(c.y), l.z.min(c.z)), Vec3::new(h.x.max(c.x), h.y.max(c.y), h.z.max(c.z)))
        });
        let ext = hi - lo;
        let key = |c: Vec3| {
            if ext.x >= ext.y && ext.x >= ext.z {
                (c.x, c.y, c.z)
            } else if ext.y >= ext.z {
                (c.y, c.z, c.x)
            } else {
                (c.z, c.x, c.y)
            }
        };
        let mut order = cs.clone();
        order.sort_by(|a, b| key(a.1).partial_cmp(&key(b.1)).unwrap_or(std::cmp::Ordering::Equal));
        for (k, (j, _)) in order.iter().enumerate() {
            if let Some(slot) = names.get_mut(*j) {
                *slot = format!("{n}{mark}{}", k + 1);
            }
        }
    }
}

/// The name of a face new to `f`.
fn role_name(f: &Feature, before: &ModelState, ps: &[Vec3], ns: &[Vec3], fi: usize, fid: u64) -> String {
    let c = ps.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / ps.len().max(1) as f64);
    match &f.kind {
        FeatureKind::Extrude { sketch, .. } => {
            let Some(ss) = before.sketch(*sketch) else { return format!("F{fid}:face:{fi}") };
            let n = ss.plane.normal();
            // A face across the extrusion is a cap; others sweep a sketch curve.
            let flat = !ns.is_empty() && ns.iter().all(|m| m.cross(n).len() < 1e-6);
            if flat {
                // On the sketch plane, above it or below it.
                let h = (c - ss.plane.origin).dot(n);
                let tol = 1e-6 * (1.0 + h.abs());
                return format!(
                    "F{fid}:{}",
                    if h.abs() <= tol {
                        "start"
                    } else if h > 0.0 {
                        "top"
                    } else {
                        "bottom"
                    }
                );
            }
            let q = ss.plane.to_local(c);
            nearest_curve(ss, Vec2::new(q.x, q.y)).map(|id| format!("F{fid}:side:{id}")).unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Revolve { sketch, axis, .. } => {
            let Some(ss) = before.sketch(*sketch) else { return format!("F{fid}:face:{fi}") };
            let Ok((o2, d2)) = crate::eval::revolve_axis(ss, axis) else { return format!("F{fid}:face:{fi}") };
            let (o, d) = (ss.plane.to_world(o2), (ss.plane.to_world(o2 + d2) - ss.plane.to_world(o2)).normalized().unwrap_or(Vec3::Z));
            // A flat face holding the axis is an angle cap: on the sketch plane, or the far one.
            let flat = !ns.is_empty() && ns.windows(2).all(|w| w[0].cross(w[1]).len() < 1e-6) && ns.iter().all(|m| m.dot(d).abs() < 1e-6);
            if flat {
                let on_plane = ps.iter().all(|p| (*p - ss.plane.origin).dot(ss.plane.normal()).abs() < 1e-6 * (1.0 + p.len()));
                return format!("F{fid}:{}", if on_plane { "start" } else { "end" });
            }
            // A swept face: turn its centre back into the sketch plane and find the curve.
            let q = c - o;
            let t = q.dot(d);
            let r = (q - d * t).len();
            let u3 = ss.plane.normal().cross(d).normalized().unwrap_or(Vec3::X);
            let best = [u3, u3 * -1.0]
                .iter()
                .filter_map(|u| {
                    let w = ss.plane.to_local(o + d * t + *u * r);
                    let q2 = Vec2::new(w.x, w.y);
                    nearest_curve_dist(ss, q2)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            best.map(|(_, id)| format!("F{fid}:side:{id}")).unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Sweep { .. } | FeatureKind::Loft { .. } => {
            let sk = match &f.kind {
                FeatureKind::Sweep { sketch, .. } => Some(*sketch),
                FeatureKind::Loft { sections, .. } => sections.first().map(|x| x.sketch),
                _ => None,
            };
            let Some(ss) = sk.and_then(|k| before.sketch(k)) else { return format!("F{fid}:face:{fi}") };
            let n = ss.plane.normal();
            let h = |p: &Vec3| (*p - ss.plane.origin).dot(n);
            let flat = !ns.is_empty() && ns.windows(2).all(|w| w[0].cross(w[1]).len() < 1e-6);
            if flat && ps.iter().all(|p| h(p).abs() < 1e-6 * (1.0 + p.len())) {
                return format!("F{fid}:start");
            }
            if flat && ns.iter().all(|m| m.cross(n).len() < 1e-6) {
                return format!("F{fid}:end");
            }
            // A side: the profile curve under its sample nearest the start.
            let Some(p0) = ps.iter().min_by(|a, b| h(a).abs().total_cmp(&h(b).abs())) else { return format!("F{fid}:face:{fi}") };
            let q = ss.plane.to_local(*p0);
            nearest_curve(ss, Vec2::new(q.x, q.y)).map(|id| format!("F{fid}:side:{id}")).unwrap_or_else(|| format!("F{fid}:face:{fi}"))
        }
        FeatureKind::Box { .. } => {
            let n = ns.first().copied().unwrap_or(Vec3::Z);
            let axis = |v: f64, p: &str, m: &str| {
                if v > 0.5 {
                    Some(p.to_string())
                } else if v < -0.5 {
                    Some(m.to_string())
                } else {
                    None
                }
            };
            let role =
                axis(n.x, "+x", "-x").or_else(|| axis(n.y, "+y", "-y")).or_else(|| axis(n.z, "+z", "-z")).unwrap_or_else(|| format!("face:{fi}"));
            format!("F{fid}:{role}")
        }
        FeatureKind::Cylinder { axis, .. } => {
            let flat = !ns.is_empty() && ns.iter().all(|m| m.cross(*axis).len() < 1e-6);
            let role = if !flat {
                "side"
            } else if ns.first().is_some_and(|m| m.dot(*axis) > 0.0) {
                "top"
            } else {
                "bottom"
            };
            format!("F{fid}:{role}")
        }
        FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => {
            let k = edges.iter().enumerate().min_by(|a, b| a.1.dist(c).total_cmp(&b.1.dist(c))).map(|(k, _)| k).unwrap_or(0);
            match f.edge_names.get(k) {
                Some(e) => format!("F{fid}:blend:{e}"),
                None => format!("F{fid}:blend:{k}"),
            }
        }
        _ => format!("F{fid}:face:{fi}"),
    }
}

/// The sketch curve (id) nearest a point in the sketch plane, and how near.
fn nearest_curve_dist(ss: &crate::SolvedSketch, q: Vec2) -> Option<(f64, String)> {
    let mut best: Option<(f64, String)> = None;
    for (ci, c) in ss.sketch.curves.iter().enumerate() {
        if c.construction {
            continue;
        }
        for s in ss.sketch.segs(ci) {
            let d = seg_dist(&s, q);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, c.id.clone()));
            }
        }
    }
    best
}

/// The sketch curve (id) nearest a point in the sketch plane.
fn nearest_curve(ss: &crate::SolvedSketch, q: Vec2) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for (ci, c) in ss.sketch.curves.iter().enumerate() {
        if c.construction {
            continue;
        }
        for s in ss.sketch.segs(ci) {
            let d = seg_dist(&s, q);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, c.id.clone()));
            }
        }
    }
    best.map(|(_, id)| id)
}

fn seg_dist(s: &Seg2, q: Vec2) -> f64 {
    let pts = s.polyline(1e-3);
    pts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            let t = if d.len2() > 0.0 { ((q - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            (a + d * t).dist(q)
        })
        .fold(f64::MAX, f64::min)
}

/// Names of a body's edges (by mesh edge index): `<face A>|<face B>`, numbered when repeated.
pub fn edge_names(b: &ModelBody) -> Vec<String> {
    let m = b.mesh();
    let faces = face_names(b);
    let mut names: Vec<String> = m
        .edge_faces
        .iter()
        .map(|fs| {
            let mut n: Vec<&str> = fs.iter().filter_map(|f| faces.get(*f as usize).map(String::as_str)).collect();
            n.sort();
            n.join("|")
        })
        .collect();
    let pts: Vec<Vec<Vec3>> = m.edges.clone();
    number_pieces(&mut names, &pts, '~');
    names
}

/// The edge of a body with this name: exact, else the pieces of the same edge nearest `near`
/// (`Err` explains when there is none).
pub fn find_edge(b: &ModelBody, name: &str, near: Vec3) -> Result<(Vec3, bool), String> {
    let m = b.mesh();
    let names = edge_names(b);
    let mid = |i: usize| -> Option<Vec3> {
        let e = m.edges.get(i)?;
        let total: f64 = e.windows(2).map(|w| w[0].dist(w[1])).sum();
        let mut acc = 0.0;
        for w in e.windows(2) {
            let l = w[0].dist(w[1]);
            if acc + l >= total / 2.0 {
                let t = if l > 0.0 { (total / 2.0 - acc) / l } else { 0.0 };
                return Some(w[0] + (w[1] - w[0]) * t);
            }
            acc += l;
        }
        e.first().copied()
    };
    if let Some(i) = names.iter().position(|n| n == name) {
        // Keep the picked point when it is on that edge.
        let on = m.edges.get(i).is_some_and(|e| e.windows(2).any(|w| near.dist_to_segment(w[0], w[1]) < (b.body.size() * 2e-3).max(1e-3)));
        return mid(i).map(|p| (if on { near } else { p }, false)).ok_or_else(|| "edge".into());
    }
    let stem = strip_edge_piece(name);
    let cands: Vec<usize> = (0..names.len()).filter(|i| names.get(*i).is_some_and(|n| strip_edge_piece(n) == stem)).collect();
    cands
        .iter()
        .filter_map(|i| mid(*i))
        .min_by(|a, b| a.dist(near).total_cmp(&b.dist(near)))
        .map(|p| (p, true))
        .ok_or_else(|| format!("edge `{name}` no longer exists"))
}

/// Names of the edges nearest each point on a body (for storing references).
pub fn edge_names_at(b: &ModelBody, pts: &[Vec3]) -> Vec<String> {
    let m = b.mesh();
    let names = edge_names(b);
    pts.iter()
        .map(|p| {
            m.edges
                .iter()
                .enumerate()
                .map(|(i, e)| (e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min), i))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .and_then(|(_, i)| names.get(i).cloned())
                .unwrap_or_default()
        })
        .collect()
}

/// Names of the faces holding each point (any body; empty when none does).
pub fn face_names_at(st: &ModelState, pts: &[Vec3]) -> Vec<String> {
    pts.iter()
        .map(|p| {
            for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
                let tol = (b.body.size() * 1e-3).max(1e-3);
                let idx = FaceIndex::new(&b.mesh(), face_count(b));
                if let Some(f) = idx.face_at(*p, tol) {
                    return face_names(b).get(f).cloned().unwrap_or_default();
                }
            }
            String::new()
        })
        .collect()
}

/// The point on the face with this name nearest `p` (a split face: its nearest piece, with
/// `true` for "it was split"); `None` when no face has the name.
pub fn point_on_face(st: &ModelState, name: &str, p: Vec3) -> Option<(Vec3, bool)> {
    let stem = strip_piece(name);
    let mut best: Option<(f64, Vec3, bool)> = None;
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let names = face_names(b);
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some(n) = names.get(*f as usize) else { continue };
            let exact = n == name;
            if !exact && strip_piece(n) != stem {
                continue;
            }
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let q = closest(p, a, bb, c);
            let d = q.dist(p) - if exact { 1e-9 } else { 0.0 };
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, q, !exact));
            }
        }
    }
    best.map(|(_, q, split)| (q, split))
}
