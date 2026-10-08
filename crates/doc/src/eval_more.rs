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
            let mut loops = face_loops(&mesh, &plane, *face, n, lo, tol);
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

/// Outline loops (in `plane` coordinates) of the mesh triangles lying in the plane at distance
/// `level` from `at` along `n`: the edges used by one of them only, chained.
fn face_loops(mesh: &solvecraft_geom::Mesh, plane: &Plane, at: Vec3, n: Vec3, level: f64, tol: f64) -> Vec<Vec<Vec2>> {
    let on: Vec<[u32; 3]> = mesh
        .triangles
        .iter()
        .copied()
        .filter(|t| t.iter().all(|k| mesh.positions.get(*k as usize).is_some_and(|p| ((*p - at).dot(n) - level).abs() < tol)))
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
    loops
}

/// The body a placed plastic feature joins: by name, else the one under the point.
fn body_under(st: &ModelState, body: &Option<String>, p: Vec3) -> Result<String> {
    let i = body_at(st, body, &[p])?;
    st.bodies.get(i).map(|b| b.name.clone()).ok_or_else(|| DocError::Invalid("there is no body".into()))
}

/// Axes with z along `z` (and x along `x` when given).
fn local_frame(z: Vec3, x: Option<Vec3>) -> Result<(Vec3, Vec3, Vec3)> {
    let z = z.normalized().ok_or_else(|| DocError::Invalid("direction".into()))?;
    let x = x.and_then(|x| (x - z * x.dot(z)).normalized()).unwrap_or_else(|| z.any_perp());
    let y = z.cross(x);
    Ok((x, y, z))
}

/// Offset a closed polygon to its left by `d` (mitred corners).
fn offset_left(p: &[Vec2], d: f64) -> Vec<Vec2> {
    let m = p.len();
    let line = |i: usize| -> Option<(Vec2, Vec2)> {
        let (a, b) = (*p.get(i)?, *p.get((i + 1) % m)?);
        let t = (b - a).normalized()?;
        Some((a + Vec2::new(-t.y, t.x) * d, t))
    };
    (0..m)
        .filter_map(|i| {
            let (p0, d0) = line((i + m - 1) % m)?;
            let (p1, d1) = line(i)?;
            let den = d0.cross(d1);
            if den.abs() < 1e-12 {
                return Some(p1);
            }
            Some(p0 + d0 * ((p1 - p0).cross(d1) / den))
        })
        .collect()
}

pub(super) fn plastic_eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let len = |e: &str| val(vals, e, Kind::Length);
    // The plastic rule of the body a feature goes on gives its defaults (draft, clearance).
    let rule = |target: &str| -> Result<Option<crate::plastic::PlasticValues>> {
        doc.plastic_rule_for(target).map(|r| doc.plastic_values(vals, &r)).transpose()
    };
    // Tools start a little inside the material so they overlap it instead of touching a face.
    match &f.kind {
        FeatureKind::Boss {
            position,
            direction,
            diameter,
            height,
            hole_diameter,
            hole_depth,
            draft,
            fillet,
            ribs,
            rib_thickness,
            rib_length,
            rib_offset,
            body,
        } => {
            let target = body_under(st, body, *position)?;
            let (r0, h) = (len(diameter)? / 2.0, len(height)?);
            if !(r0 > 0.0 && h > 0.0) {
                return Err(DocError::Invalid("the boss diameter and height must be positive".into()));
            }
            let dr = match draft {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => rule(&target)?.map_or(0.0, |r| r.draft),
            };
            let rf = match fillet {
                Some(e) => len(e)?.max(0.0),
                None => 0.0,
            };
            let r_top = r0 - h * dr.tan();
            if r_top <= 0.0 {
                return Err(DocError::Invalid("the draft closes the boss before its top".into()));
            }
            let delta = (h * 0.05).clamp(1e-3, 0.5);
            let (x, _, z) = local_frame(*direction, None)?;
            // The post: a (tapered) cylinder from a little inside the face.
            let base = Plane::new(*position - z * delta, x, z.cross(x)).ok_or_else(|| DocError::Invalid("boss frame".into()))?;
            let r_in = r0 + delta * dr.tan();
            let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, r_in), holes: Vec::new() };
            let mut tools = if dr.abs() > 1e-12 {
                vec![kernel::extrude_tapered(&base, &disc, h + delta, 1.0, -dr)?]
            } else {
                kernel::extrude(&base, &[disc], 0.0, h + delta)?
            };
            if let Some(n) = ribs {
                let n = val(vals, n, Kind::Unitless)?.round();
                if !(0.0..=64.0).contains(&n) {
                    return Err(DocError::Invalid("0…64 ribs".into()));
                }
                let t = match rib_thickness {
                    Some(e) => len(e)?,
                    None => r0 * 0.3,
                };
                let l = match rib_length {
                    Some(e) => len(e)?,
                    None => r0,
                };
                let off = match rib_offset {
                    Some(e) => len(e)?,
                    None => 0.0,
                };
                let hr = h - off;
                if n > 0.0 && !(t > 0.0 && l > 0.0 && hr > 0.0) {
                    return Err(DocError::Invalid("ribs need a positive thickness, length and height".into()));
                }
                for k in 0..n as usize {
                    let a = std::f64::consts::TAU * k as f64 / n;
                    let (u, v) = (x * a.cos() + z.cross(x) * a.sin(), z.cross(x) * a.cos() - x * a.sin());
                    let rp = Plane::new(*position, u, v).ok_or_else(|| DocError::Invalid("rib frame".into()))?;
                    let rect = Region2 {
                        outer: Loop2::polygon(&[
                            Vec2::new(r0 * 0.5, -t / 2.0),
                            Vec2::new(r0 + l, -t / 2.0),
                            Vec2::new(r0 + l, t / 2.0),
                            Vec2::new(r0 * 0.5, t / 2.0),
                        ]),
                        holes: Vec::new(),
                    };
                    tools.extend(kernel::extrude(&rp, &[rect], -delta, hr)?);
                }
            }
            apply_op(st, f, tools, Operation::Join, std::slice::from_ref(&target))?;
            // The root fillet: rounds the circle where the post meets the face.
            if rf > 1e-9 {
                let i = st.bodies.iter().position(|b| b.name == target).ok_or_else(|| DocError::Invalid("boss body".into()))?;
                let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("boss body".into())) };
                let mut warn = None;
                let edges = resolve_edges(&mb.body, &[*position + x * r0], &mut warn)?;
                let nb = kernel::fillet(&mb.body, &edges, rf)?;
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = ModelBody::new(mb.name, nb, mb.feature);
                }
            }
            if let Some(hd) = hole_diameter {
                let rh = len(hd)? / 2.0;
                let depth = match hole_depth {
                    Some(e) => len(e)?,
                    None => h,
                };
                if !(rh > 0.0 && rh < r_top && depth > 0.0) {
                    return Err(DocError::Invalid("the boss hole must fit inside the boss".into()));
                }
                let hp = Plane::new(*position, x, z.cross(x)).ok_or_else(|| DocError::Invalid("hole frame".into()))?;
                let tool = kernel::extrude(&hp, &[Region2 { outer: Loop2::circle(Vec2::ZERO, rh), holes: Vec::new() }], h - depth, h + delta)?;
                apply_op(st, f, tool, Operation::Cut, std::slice::from_ref(&target))?;
            }
            Ok(())
        }
        FeatureKind::Lip { face, width, height, groove, gap, outside, body } => {
            let target = body_under(st, body, *face)?;
            let mb = st.body(&target).cloned().ok_or_else(|| DocError::Invalid("body".into()))?;
            let n = planar_normal_at(&mb.body, *face).ok_or_else(|| DocError::Invalid("pick the planar rim face".into()))?;
            let (w, h) = (len(width)?, len(height)?);
            let g = match gap {
                Some(e) => len(e)?,
                None if *groove => rule(&target)?.map_or(0.0, |r| r.clearance),
                None => 0.0,
            };
            if !(w > 0.0 && h > 0.0 && g >= 0.0) {
                return Err(DocError::Invalid("the lip width and height must be positive".into()));
            }
            let plane = Plane::from_normal(*face, n).ok_or_else(|| DocError::Invalid("rim plane".into()))?;
            let tol = (mb.body.size() * 1e-3).max(1e-3);
            let mesh = mb.body.tessellate(tol)?;
            let mut loops = face_loops(&mesh, &plane, *face, n, 0.0, tol);
            loops.sort_by(|a, b| poly_area(b).abs().total_cmp(&poly_area(a).abs()));
            let ccw = |l: &[Vec2]| if poly_area(l) < 0.0 { l.iter().rev().copied().collect::<Vec<_>>() } else { l.to_vec() };
            let (Some(outer), Some(inner)) = (loops.first().map(|l| ccw(l)), loops.get(1).map(|l| ccw(l))) else {
                return Err(DocError::Invalid("pick the rim face of a shelled body (a face with an inner and an outer edge)".into()));
            };
            let bw = if *groove { w + g } else { w };
            // The band along the chosen edge, inside the rim face.
            let band = if *outside {
                Region2 { outer: Loop2::polygon(&outer).ccw(), holes: vec![Loop2::polygon(&offset_left(&outer, bw)).ccw().reversed()] }
            } else {
                Region2 { outer: Loop2::polygon(&offset_left(&inner, -bw)).ccw(), holes: vec![Loop2::polygon(&inner).ccw().reversed()] }
            };
            let delta = (h * 0.05).clamp(1e-3, 0.5);
            if *groove {
                let tool = kernel::extrude(&plane, &[band], -(h + g), delta)?;
                apply_op(st, f, tool, Operation::Cut, std::slice::from_ref(&target))
            } else {
                let tool = kernel::extrude(&plane, &[band], -delta, h)?;
                apply_op(st, f, tool, Operation::Join, std::slice::from_ref(&target))
            }
        }
        FeatureKind::SnapFit { position, direction, hook, length, thickness, width, catch_depth, catch_length, body } => {
            let target = body_under(st, body, *position)?;
            let (l, t, w, cd, cl) = (len(length)?, len(thickness)?, len(width)?, len(catch_depth)?, len(catch_length)?);
            if !(l > 0.0 && t > 0.0 && w > 0.0 && cd > 0.0 && cl > 0.0 && cl < l) {
                return Err(DocError::Invalid("the snap fit's sizes must be positive and the catch shorter than the arm".into()));
            }
            let (x, y, z) = local_frame(*direction, Some(*hook))?;
            let delta = (l * 0.05).clamp(1e-3, 0.5);
            // Side profile (x = toward the catch, y = up the arm), extruded across the width.
            let pts =
                [Vec2::new(-t, -delta), Vec2::new(0.0, -delta), Vec2::new(0.0, l - cl), Vec2::new(cd, l - cl), Vec2::new(0.0, l), Vec2::new(-t, l)];
            let o = *position - y * (w / 2.0);
            let pl = Plane::new(o, x, z).ok_or_else(|| DocError::Invalid("snap fit frame".into()))?;
            let mut lp = Loop2::polygon(&pts);
            if lp.signed_area() < 0.0 {
                lp = lp.reversed();
            }
            let sgn = if pl.normal().dot(y) >= 0.0 { 1.0 } else { -1.0 };
            let (lo, hi) = if sgn > 0.0 { (0.0, w) } else { (-w, 0.0) };
            let tool = kernel::extrude(&pl, &[Region2 { outer: lp, holes: Vec::new() }], lo, hi)?;
            apply_op(st, f, tool, Operation::Join, std::slice::from_ref(&target))
        }
        FeatureKind::Rest { position, direction, along, width, length, height, draft, thickness, body } => {
            let target = body_under(st, body, *position)?;
            let (w, h) = (len(width)?, len(height)?);
            let l = length.as_deref().map(len).transpose()?;
            let dr = match draft {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => rule(&target)?.map_or(0.0, |r| r.draft),
            };
            let t = thickness.as_deref().map(len).transpose()?;
            if !(w > 0.0 && h > 0.0 && l.is_none_or(|l| l > 0.0) && t.is_none_or(|t| t > 0.0 && 2.0 * t < w.min(l.unwrap_or(w)))) {
                return Err(DocError::Invalid("the rest's sizes must be positive and its wall thinner than half its width".into()));
            }
            if !(dr.abs() < 1.0) {
                return Err(DocError::Invalid("the rest's draft must be under 57°".into()));
            }
            let shrink = h * dr.tan();
            let closes = match t {
                Some(t) => 2.0 * shrink >= t,
                None => 2.0 * shrink >= w.min(l.unwrap_or(w)),
            };
            if closes {
                return Err(DocError::Invalid("the draft closes the rest before its top".into()));
            }
            let delta = (h * 0.05).clamp(1e-3, 0.5);
            let (x, y, z) = local_frame(*direction, *along)?;
            // Sized at the face: the tool starts a little inside, grown by the draft over that.
            let e = delta * dr.tan();
            let shape = |grow: f64| match l {
                Some(l) => Loop2::polygon(&[
                    Vec2::new(-l / 2.0 - grow, -w / 2.0 - grow),
                    Vec2::new(l / 2.0 + grow, -w / 2.0 - grow),
                    Vec2::new(l / 2.0 + grow, w / 2.0 + grow),
                    Vec2::new(-l / 2.0 - grow, w / 2.0 + grow),
                ]),
                None => Loop2::circle(Vec2::ZERO, w / 2.0 + grow),
            };
            let holes = match t {
                Some(t) => vec![shape(-t - e).reversed()],
                None => Vec::new(),
            };
            let region = Region2 { outer: shape(e), holes };
            let base = Plane::new(*position - z * delta, x, y).ok_or_else(|| DocError::Invalid("rest frame".into()))?;
            let tool = if dr.abs() > 1e-12 {
                vec![kernel::extrude_tapered(&base, &region, h + delta, 1.0, -dr)?]
            } else {
                kernel::extrude(&base, &[region], 0.0, h + delta)?
            };
            apply_op(st, f, tool, Operation::Join, std::slice::from_ref(&target))
        }
        _ => Err(DocError::Invalid(format!("{} is not a plastic feature", f.name))),
    }
}
