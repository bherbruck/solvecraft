//! Evaluation of Emboss, Rib/Web, Replace Face, Align and Remove (features built from the
//! kernel's extrude, boolean, offset and transform operations).

use super::*;
use crate::apply_point;
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

/// Rebuild a sheet body's solid from its model.
fn rebuild_sheet(st: &mut ModelState, idx: usize) -> Result<()> {
    let sheet = st.sheets.get(idx).cloned().ok_or_else(|| DocError::Invalid("sheet".into()))?;
    let solid = sheet.solid()?;
    let i = st.bodies.iter().position(|b| b.name == sheet.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", sheet.body)))?;
    if let Some(mb) = st.bodies.get_mut(i) {
        *mb = ModelBody::new(mb.name.clone(), solid, mb.feature);
    }
    Ok(())
}

/// The sheet a feature works on: by body name, else the one with an edge nearest the pick.
fn sheet_for(st: &ModelState, body: &Option<String>, pick: Option<Vec3>) -> Result<usize> {
    if let Some(n) = body {
        return st.sheets.iter().position(|s| &s.body == n).ok_or_else(|| DocError::Invalid(format!("`{n}` is not a sheet metal body")));
    }
    let p = pick.ok_or_else(|| DocError::Invalid("no sheet metal body".into()))?;
    st.sheets
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.edge_at(p).map(|e| (i, e.4)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| DocError::Invalid("there is no sheet metal body".into()))
}

/// A region mirrored in the sketch x axis (y → −y); curves other than lines and arcs as polylines.
fn mirror_y(r: &Region2) -> Region2 {
    let f = |p: Vec2| Vec2::new(p.x, -p.y);
    let m = |l: &Loop2| {
        let segs: Vec<Seg2> = l
            .segs
            .iter()
            .flat_map(|s| match *s {
                Seg2::Line { a, b } => vec![Seg2::Line { a: f(a), b: f(b) }],
                Seg2::Arc { center, radius, start, sweep } => vec![Seg2::Arc { center: f(center), radius, start: -start, sweep: -sweep }],
                other => {
                    let pts: Vec<Vec2> = (0..=16).map(|k| f(other.point_at(k as f64 / 16.0))).collect();
                    pts.windows(2).map(|w| Seg2::Line { a: w[0], b: w[1] }).collect()
                }
            })
            .collect();
        Loop2 { segs }.ccw()
    };
    Region2 { outer: m(&r.outer), holes: r.holes.iter().map(|h| m(h).reversed()).collect() }
}

pub(super) fn sheet_eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    use crate::sheet;
    match &f.kind {
        FeatureKind::SheetBase { sketch, profiles, rule, flip } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            let regions = solvecraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let r = doc.sheet_rule(rule.as_deref());
            let rv = doc.rule_values(vals, &r)?;
            for (k, region) in regions.into_iter().enumerate() {
                let (plane, region) = if *flip {
                    (Plane { origin: ss.plane.origin, x: ss.plane.x, y: ss.plane.y * -1.0 }, mirror_y(&region))
                } else {
                    (ss.plane, region)
                };
                let name = new_name(st, f, k);
                let sh = sheet::base_flange(&name, &r.name, &rv, plane, region);
                let solid = sh.solid()?;
                st.bodies.push(ModelBody::new(name, solid, f.id));
                st.sheets.push(sh);
            }
            Ok(())
        }
        FeatureKind::SheetContour { sketch, curves, distance, rule, flip, reverse } => {
            let ss = st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            for id in curves {
                let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
                if !ss.sketch.segs(ci).iter().all(|s| matches!(s, Seg2::Line { .. })) {
                    return Err(DocError::Invalid("not supported yet: arcs in a contour flange (use lines; the bends are made for you)".into()));
                }
            }
            let pts = chain_points(&ss, curves)?;
            let w = val(vals, distance, Kind::Length)?;
            if !(w > 0.0) {
                return Err(DocError::Invalid("the contour flange distance must be positive".into()));
            }
            let r = doc.sheet_rule(rule.as_deref());
            let rv = doc.rule_values(vals, &r)?;
            let plane = if *reverse { Plane { origin: ss.plane.origin, x: ss.plane.x, y: ss.plane.y * -1.0 } } else { ss.plane };
            let pts: Vec<Vec2> = if *reverse { pts.iter().map(|p| Vec2::new(p.x, -p.y)).collect() } else { pts };
            // Mirroring the sketch swaps the material side too.
            let name = new_name(st, f, 0);
            let sh = sheet::contour_flange(&name, &r.name, &rv, &plane, &pts, w, *flip != *reverse)?;
            let solid = sh.solid()?;
            st.bodies.push(ModelBody::new(name, solid, f.id));
            st.sheets.push(sh);
            Ok(())
        }
        FeatureKind::SheetFlange { edges, height, angle, radius, position, flip, body } => {
            let i = sheet_for(st, body, edges.first().copied())?;
            let (h, a) = (val(vals, height, Kind::Length)?, val(vals, angle, Kind::Angle)?);
            let rr = match radius {
                Some(e) => val(vals, e, Kind::Length)?,
                None => {
                    let rule = st.sheets.get(i).map(|s| s.rule.clone()).unwrap_or_default();
                    doc.rule_values(vals, &doc.sheet_rule(Some(&rule)))?.bend_radius
                }
            };
            let sh = st.sheets.get_mut(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sh.flat {
                return Err(DocError::Invalid("refold the sheet before adding flanges".into()));
            }
            sh.add_flanges(edges, h, a, rr, position, *flip)?;
            rebuild_sheet(st, i)
        }
        FeatureKind::SheetHem { edges, length, gap, flip, body } => {
            let i = sheet_for(st, body, edges.first().copied())?;
            let l = val(vals, length, Kind::Length)?;
            let g = match gap {
                Some(e) => val(vals, e, Kind::Length)?,
                None => {
                    let rule = st.sheets.get(i).map(|s| s.rule.clone()).unwrap_or_default();
                    doc.rule_values(vals, &doc.sheet_rule(Some(&rule)))?.hem_gap
                }
            };
            let sh = st.sheets.get_mut(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            for e in edges {
                sh.add_hem(*e, l, g, *flip)?;
            }
            rebuild_sheet(st, i)
        }
        FeatureKind::SheetUnfold { body, refold } => {
            let i = match body {
                Some(_) => sheet_for(st, body, None)?,
                None => st.sheets.iter().rposition(|s| s.flat == *refold).ok_or_else(|| {
                    DocError::Invalid(if *refold { "no unfolded sheet to refold".into() } else { "no sheet metal body to unfold".into() })
                })?,
            };
            if let Some(sh) = st.sheets.get_mut(i) {
                sh.flat = !*refold;
            }
            rebuild_sheet(st, i)
        }
        FeatureKind::SheetConvert { body, face, rule } => {
            let mb = st.body(body).cloned().ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let n = planar_normal_at(&mb.body, *face).ok_or_else(|| DocError::Invalid("pick a planar face of the plate".into()))?;
            let tol = (mb.body.size() * 1e-3).max(1e-3);
            let mesh = mb.body.tessellate(tol)?;
            // Thickness: the body's extent along the face normal.
            let ds: Vec<f64> = mesh.positions.iter().map(|p| (*p - *face).dot(n)).collect();
            let (lo, hi) = (ds.iter().cloned().fold(f64::MAX, f64::min), ds.iter().cloned().fold(f64::MIN, f64::max));
            let t = hi - lo;
            // The face outline: boundary edges of the triangles on that face's plane.
            let plane = Plane::from_normal(*face + n * lo, n * -1.0).ok_or_else(|| DocError::Invalid("face".into()))?;
            let on: Vec<[u32; 3]> = mesh
                .triangles
                .iter()
                .copied()
                .filter(|t| t.iter().all(|k| mesh.positions.get(*k as usize).is_some_and(|p| ((*p - *face).dot(n) - lo).abs() < tol)))
                .collect();
            // Boundary edges by position (meshes may repeat vertices per triangle).
            let pos = |k: u32| mesh.positions.get(k as usize).copied().unwrap_or_default();
            let q = |p: Vec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
            let mut count: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), i32> = Default::default();
            let edges: Vec<(Vec3, Vec3)> = on.iter().flat_map(|t| [(pos(t[0]), pos(t[1])), (pos(t[1]), pos(t[2])), (pos(t[2]), pos(t[0]))]).collect();
            for (a, b) in &edges {
                let (ka, kb) = (q(*a), q(*b));
                *count.entry((ka.min(kb), ka.max(kb))).or_insert(0) += 1;
            }
            let mut boundary: Vec<(Vec3, Vec3)> = edges
                .into_iter()
                .filter(|(a, b)| {
                    let (ka, kb) = (q(*a), q(*b));
                    count.get(&(ka.min(kb), ka.max(kb))) == Some(&1)
                })
                .collect();
            // Chain into loops (plane coordinates).
            let mut loops: Vec<Vec<Vec2>> = Vec::new();
            for _ in 0..10_000 {
                let Some((s0, mut cur)) = boundary.pop() else { break };
                let mut lp = vec![s0];
                for _ in 0..boundary.len() + 1 {
                    if q(cur) == q(s0) {
                        break;
                    }
                    lp.push(cur);
                    match boundary.iter().position(|(a, _)| q(*a) == q(cur)) {
                        Some(k) => cur = boundary.remove(k).1,
                        None => break,
                    }
                }
                let raw: Vec<Vec2> = lp.iter().map(|p| plane.to_local(*p)).collect();
                // Drop points in the middle of straight runs.
                let m = raw.len();
                let pts: Vec<Vec2> = (0..m)
                    .filter_map(|i| {
                        let (p, c, n) = (*raw.get((i + m - 1) % m)?, *raw.get(i)?, *raw.get((i + 1) % m)?);
                        ((c - p).cross(n - c).abs() > 1e-9 * (c - p).len().max(1e-9) * (n - c).len().max(1e-9)).then_some(c)
                    })
                    .collect();
                if pts.len() >= 3 {
                    loops.push(pts);
                }
            }
            loops.sort_by(|a, b| poly_area(b).abs().total_cmp(&poly_area(a).abs()));
            let mut it = loops.into_iter();
            let outer = Loop2::polygon(&it.next().ok_or_else(|| DocError::Invalid("no outline on that face".into()))?).ccw();
            let region = Region2 { outer, holes: it.map(|l| Loop2::polygon(&l).ccw().reversed()).collect() };
            let area = region.area().abs();
            let vol = kernel::measure(&mb.body)?.volume;
            if (area * t - vol).abs() > 1e-3 * vol.max(1.0) {
                return Err(DocError::Invalid("not supported yet: converting bodies that are not a plate of even thickness".into()));
            }
            let r = doc.sheet_rule(rule.as_deref());
            let mut rv = doc.rule_values(vals, &r)?;
            rv.thickness = t;
            let mut sh = sheet::base_flange(body, &r.name, &rv, plane, Region2 { outer: region.outer.clone(), holes: Vec::new() });
            sh.holes = region.holes.iter().map(|h| Region2 { outer: h.ccw(), holes: Vec::new() }).collect();
            let solid = sh.solid()?;
            if let Some(b) = st.bodies.iter_mut().find(|b| &b.name == body) {
                *b = ModelBody::new(b.name.clone(), solid, b.feature);
            }
            st.sheets.retain(|s| &s.body != body);
            st.sheets.push(sh);
            Ok(())
        }
        _ => Err(DocError::Invalid(format!("{} is not a sheet metal feature", f.name))),
    }
}

fn poly_area(p: &[Vec2]) -> f64 {
    let m = p.len();
    (0..m).filter_map(|i| Some(p.get(i)?.cross(*p.get((i + 1) % m)?))).sum::<f64>() / 2.0
}

/// An extrude cut through sheet metal: the profiles become cut-outs of the flat pattern
/// (parallel to a panel, through its thickness). Returns the sheet bodies handled.
pub(super) fn sheet_cut(f: &Feature, st: &mut ModelState, tools: &[Body], targets: &[String]) -> Result<Vec<String>> {
    let FeatureKind::Extrude { sketch, profiles, .. } = &f.kind else { return Ok(Vec::new()) };
    let Some(ss) = st.sketch(*sketch).cloned() else { return Ok(Vec::new()) };
    let regions = solvecraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
    let n = ss.plane.normal();
    // How far the tools reach along the sketch normal.
    let mut span = (f64::MAX, f64::MIN);
    for t in tools {
        for p in t.tessellate((t.size() * 1e-3).max(1e-3))?.positions {
            let d = (p - ss.plane.origin).dot(n);
            span = (span.0.min(d), span.1.max(d));
        }
    }
    let mut handled = Vec::new();
    for i in 0..st.sheets.len() {
        let Some(sh) = st.sheets.get(i).cloned() else { continue };
        if !targets.is_empty() && !targets.contains(&sh.body) {
            continue;
        }
        let candidates: Vec<(Option<usize>, Region2)> = if sh.flat {
            if sh.frame.normal().cross(n).len() > 1e-6 {
                Vec::new()
            } else {
                vec![(None, Region2 { outer: Loop2::polygon(&sh.outline()?.into_iter().next().unwrap_or_default()), holes: Vec::new() })]
            }
        } else {
            sh.parallel_panels(&ss.plane)
        };
        let mut added = Vec::new();
        for (pi, panel) in candidates {
            // The panel's faces along the sketch normal must both be inside the cut.
            let w = sh.world(pi);
            let z0 = (apply_point(&w, Vec3::ZERO) - ss.plane.origin).dot(n);
            let z1 = (apply_point(&w, Vec3::Z * sh.t) - ss.plane.origin).dot(n);
            if z0.min(z1) < span.0 - 1e-6 || z0.max(z1) > span.1 + 1e-6 {
                continue;
            }
            let outline = if sh.flat { sh.outline()?.into_iter().flatten().collect::<Vec<_>>() } else { panel.outer.polyline(1e-3) };
            for r in &regions {
                let Some(flat) = sh.region_to_flat(pi, &ss.plane, r) else { continue };
                let fp = flat.outer.polyline(1e-3);
                let overlaps = fp.iter().any(|p| point_in_poly(&outline, *p)) || outline.iter().any(|p| point_in_poly(&fp, *p));
                if overlaps && !added.iter().any(|x: &Region2| x == &flat) {
                    added.push(flat);
                }
            }
        }
        if added.is_empty() {
            continue;
        }
        if let Some(s) = st.sheets.get_mut(i) {
            s.holes.extend(added);
        }
        rebuild_sheet(st, i)?;
        handled.push(sh.body.clone());
    }
    Ok(handled)
}

fn point_in_poly(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let m = poly.len();
    for i in 0..m {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + m - 1) % m)) else { continue };
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
