//! Evaluation of Emboss, Rib/Web, Replace Face, Align and Remove (features built from the
//! kernel's extrude, boolean, offset and transform operations).

use super::*;
use solvecraft_geom::{Loop2, Seg2};

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    match &f.kind {
        FeatureKind::Emboss { sketch, profiles, depth, deboss, targets } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
            let regions = select_profiles(ss, profiles)?;
            let d = val(vals, depth, Kind::Length)?;
            if !(d > 0.0) {
                return Err(DocError::Invalid("the emboss depth must be positive".into()));
            }
            let (a, b) = if *deboss { (-d, 0.0) } else { (0.0, d) };
            let tools = kernel::extrude(&ss.plane, &regions, a, b)?;
            if st.bodies.is_empty() {
                return Err(DocError::Invalid("there is no body to emboss".into()));
            }
            apply_op(st, f, tools, if *deboss { Operation::Cut } else { Operation::Join }, targets)
        }
        FeatureKind::Rib { sketch, curves, thickness, depth, flip, web } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            let t = val(vals, thickness, Kind::Length)?;
            if !(t > 0.0) {
                return Err(DocError::Invalid("the rib thickness must be positive".into()));
            }
            let depth = match depth {
                Some(e) => Some(val(vals, e, Kind::Length)?)
                    .filter(|d| *d > 0.0)
                    .ok_or_else(|| DocError::Invalid("the rib depth must be positive".into()))
                    .map(Some)?,
                None => None,
            };
            let groups: Vec<Vec<String>> = if *web { curves.iter().map(|c| vec![c.clone()]).collect() } else { vec![curves.clone()] };
            if groups.is_empty() || curves.is_empty() || curves.len() > 1000 {
                return Err(DocError::Invalid("a rib needs 1…1000 sketch curves".into()));
            }
            let mut tools = Vec::new();
            for g in &groups {
                tools.push(rib_tool(st, &ss, g, t, depth, *flip)?);
            }
            apply_op(st, f, tools, Operation::Join, &[])
        }
        FeatureKind::ReplaceFace { faces, target, body } => {
            let tp = doc.resolve_plane(vals, target, 0)?;
            let tn = tp.normal();
            let i = body_at(st, body, faces)?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mut b = mb.body.clone();
            for p in faces {
                let n = planar_normal_at(&b, *p).ok_or_else(|| DocError::Invalid("not supported yet: replacing a face that is not planar".into()))?;
                if n.dot(tn).abs() < 1.0 - 1e-9 {
                    return Err(DocError::Invalid("not supported yet: a target that is not parallel to the face".into()));
                }
                let d = (tp.origin - *p).dot(n);
                if d.abs() > 1e-9 {
                    b = kernel::offset_faces(&b, &[*p], d)?;
                }
            }
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, b, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Align { bodies, from, to, from_normal, to_normal, flip } => {
            let mut m = translation(Vec3::ZERO);
            if let (Some(a), Some(b)) = (from_normal.and_then(Vec3::normalized), to_normal.and_then(Vec3::normalized)) {
                // Turn `a` onto −b (faces meet) or b (flipped), about the `from` point.
                let want = if *flip { b } else { b * -1.0 };
                let axis = a.cross(want);
                let ang = axis.len().atan2(a.dot(want));
                let axis = axis.normalized().unwrap_or_else(|| a.any_perp());
                if ang.abs() > 1e-12 {
                    m = rotation(*from, axis, ang);
                }
            }
            let d = *to - *from;
            m[3][0] += d.x;
            m[3][1] += d.y;
            m[3][2] += d.z;
            if bodies.is_empty() {
                return Err(DocError::Invalid("no bodies to align".into()));
            }
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                if let Some(mb) = st.bodies.get_mut(i) {
                    let nb = kernel::transform_matrix(&mb.body, m)?;
                    *mb = ModelBody::new(mb.name.clone(), nb, mb.feature);
                }
            }
            Ok(())
        }
        FeatureKind::Remove { bodies } => {
            if bodies.is_empty() {
                return Err(DocError::Invalid("no bodies to remove".into()));
            }
            for n in bodies {
                if st.body(n).is_none() {
                    return Err(DocError::Unknown(format!("body `{n}`")));
                }
            }
            st.bodies.retain(|b| !bodies.contains(&b.name));
            Ok(())
        }
        _ => Err(DocError::Invalid(format!("{} is not evaluated here", f.name))),
    }
}

/// Unit outward normal of the planar face through `p`.
fn planar_normal_at(b: &Body, p: Vec3) -> Option<Vec3> {
    let tol = (b.size() * 2e-3).max(1e-3);
    let faces = b.faces(tol).ok()?;
    faces
        .iter()
        .filter_map(|f| f.plane_normal.map(|n| (n, ((p - f.centroid).dot(n)).abs())))
        .filter(|(_, d)| *d < tol * 5.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n)
}

/// The curves of a chain as one polyline in sketch coordinates, in order.
fn chain_points(ss: &SolvedSketch, ids: &[String]) -> Result<Vec<Vec2>> {
    let mut segs: Vec<Seg2> = Vec::new();
    for id in ids {
        let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
        segs.extend(ss.sketch.segs(ci));
    }
    let Some(first) = segs.first().copied() else { return Err(DocError::Invalid("no curves".into())) };
    // Orient the first segment toward the second, then chain.
    let mut chain = vec![first];
    if let Some(n) = segs.get(1)
        && (first.start().dist(n.start()) < 1e-6 || first.start().dist(n.end()) < 1e-6)
    {
        chain[0] = first.reversed();
    }
    for s in segs.iter().skip(1) {
        let prev = chain.last().map(Seg2::end).unwrap_or_default();
        let s = if s.end().dist(prev) < s.start().dist(prev) { s.reversed() } else { *s };
        if s.start().dist(prev) > 1e-5 {
            return Err(DocError::Invalid("the rib curves are not connected end to end".into()));
        }
        chain.push(s);
    }
    let mut pts = vec![chain[0].start()];
    for s in &chain {
        let n = if matches!(s, Seg2::Line { .. }) { 1 } else { 24 };
        for k in 1..=n {
            pts.push(s.point_at(k as f64 / n as f64));
        }
    }
    Ok(pts)
}

/// A rib wall: the band swept from the curves across the plane by `depth` (or to the body),
/// `t` thick about the sketch plane.
fn rib_tool(st: &ModelState, ss: &SolvedSketch, ids: &[String], t: f64, depth: Option<f64>, flip: bool) -> Result<Body> {
    let pts = chain_points(ss, ids)?;
    let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) else { return Err(DocError::Invalid("empty rib curve".into())) };
    let chord = (b - a).normalized().ok_or_else(|| DocError::Invalid("a closed or zero-length rib curve".into()))?;
    let mid = pts.get(pts.len() / 2).copied().unwrap_or(a);
    // Toward the material: the side of the curve where the bodies are (unless flipped).
    let mut n = chord.perp();
    let reach = st.bodies.iter().map(|b| b.mesh().bounds()).fold(0.0f64, |acc, bb| acc.max((bb.max - bb.min).len()));
    let probe = |side: Vec2| {
        let mut hits = 0;
        for k in 1..=20 {
            let q = ss.plane.to_world(mid + side * (reach * k as f64 / 20.0));
            if st.bodies.iter().any(|b| b.mesh().contains(q)) {
                hits += 1;
            }
        }
        hits
    };
    if probe(n * -1.0) > probe(n) {
        n = n * -1.0;
    }
    if flip {
        n = n * -1.0;
    }
    // From each point of the curves, walk across the plane until the body begins (or `depth`);
    // walls that reach the body go a little into it so they merge with it.
    let inside = |q: Vec2| {
        let w = ss.plane.to_world(q);
        st.bodies.iter().any(|b| b.mesh().contains(w))
    };
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let samples = ((len / (t.min(len) * 0.25).max(len / 400.0)).ceil() as usize).clamp(2, 400);
    let along: Vec<Vec2> = (0..=samples).map(|k| point_on_polyline(&pts, len * k as f64 / samples as f64)).collect();
    let limit = depth.unwrap_or(reach * 2.0 + 1.0);
    let step = (limit.min(reach * 2.0 + 1.0) / 400.0).max(1e-3);
    let overlap = (len / samples as f64 * 2.0).max(step * 2.0).min(t);
    let mut far = Vec::with_capacity(along.len());
    for q in &along {
        let mut hit = None;
        let mut k = 1;
        while (k as f64) * step <= limit + step && k < 100_000 {
            if inside(*q + n * (k as f64 * step)) {
                // Refine between the last outside and the first inside sample.
                let (mut lo, mut hi) = ((k - 1) as f64 * step, k as f64 * step);
                for _ in 0..30 {
                    let m = 0.5 * (lo + hi);
                    if inside(*q + n * m) { hi = m } else { lo = m }
                }
                hit = Some(hi);
                break;
            }
            k += 1;
        }
        let h = match (hit, depth) {
            (Some(h), Some(d)) if h >= d => d,
            (Some(h), _) => h + overlap,
            (None, Some(d)) => d,
            (None, None) => return Err(DocError::Invalid("the rib curves do not face a body (give a depth)".into())),
        };
        far.push(*q + n * h);
    }
    let mut poly = along.clone();
    poly.extend(far.into_iter().rev());
    let mut lp = Loop2::polygon(&poly);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    let region = Region2 { outer: lp, holes: Vec::new() };
    let tools = kernel::extrude(&ss.plane, &[region], -t / 2.0, t / 2.0)?;
    tools.into_iter().next().ok_or_else(|| DocError::Invalid("the rib made no solid".into()))
}

/// The point `d` along a polyline.
fn point_on_polyline(pts: &[Vec2], d: f64) -> Vec2 {
    let mut left = d.max(0.0);
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            return w[0].lerp(w[1], left / l);
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}
