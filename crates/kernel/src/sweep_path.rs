//! Sweeps along 3D point paths (3D sketch curves: projected, intersection and included curves).
//! The profile follows the path with a rotation-minimising frame. Smooth paths get one B-spline
//! wall per profile segment; paths with sharp corners (or profiles with holes) get ruled walls
//! between stations, mitred at the corners.

use mt::builder;
use solvecraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, p3};
use crate::build::{cap_and_close, interpolate_cubic, uniform_wire};
use crate::{KernelError, Result, guard};

/// Most path points accepted.
const MAX_PATH_POINTS: usize = 100_000;
/// Stations of a smooth sweep (the B-spline walls interpolate them).
const MAX_SMOOTH_STATIONS: usize = 120;
/// Stations of a ruled sweep.
const MAX_RULED_STATIONS: usize = 2000;
/// A turn sharper than this between path segments is a corner (radians).
const CORNER: f64 = 0.35;
/// Samples per profile segment of the smooth walls.
const SEG_SAMPLES: usize = 12;

/// A linear map of world vectors (columns) plus the station it is centred at.
#[derive(Clone, Copy)]
struct Station {
    at: Vec3,
    m: [Vec3; 3],
}

impl Station {
    fn apply(&self, v: Vec3) -> Vec3 {
        self.m[0] * v.x + self.m[1] * v.y + self.m[2] * v.z
    }
}

fn clean(path: &[Vec3]) -> Result<Vec<Vec3>> {
    if path.len() < 2 || path.len() > MAX_PATH_POINTS {
        return Err(KernelError::Invalid("a sweep path needs 2…100000 points".into()));
    }
    let mut out: Vec<Vec3> = Vec::with_capacity(path.len());
    for p in path {
        if !(p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9 && p.z.abs() < 1e9) {
            return Err(KernelError::Invalid("the sweep path has a bad point".into()));
        }
        if out.last().is_none_or(|l| l.dist(*p) > 1e-9) {
            out.push(*p);
        }
    }
    if out.len() < 2 {
        return Err(KernelError::Invalid("the sweep path has no length".into()));
    }
    Ok(out)
}

fn turn(a: Vec3, b: Vec3) -> f64 {
    match (a.normalized(), b.normalized()) {
        (Some(a), Some(b)) => a.dot(b).clamp(-1.0, 1.0).acos(),
        _ => 0.0,
    }
}

/// Points evenly spaced by arc length along the polyline (ends kept).
fn resample(p: &[Vec3], n: usize) -> Vec<Vec3> {
    let mut cum = vec![0.0];
    for w in p.windows(2) {
        let l = cum.last().copied().unwrap_or(0.0) + w[0].dist(w[1]);
        cum.push(l);
    }
    let total = cum.last().copied().unwrap_or(0.0);
    let mut out = Vec::with_capacity(n + 1);
    let mut k = 0;
    for i in 0..=n {
        let s = total * i as f64 / n as f64;
        while k + 2 < cum.len() && cum.get(k + 1).is_some_and(|c| *c < s) {
            k += 1;
        }
        let (Some(a), Some(b), Some(ca), Some(cb)) = (p.get(k), p.get(k + 1), cum.get(k), cum.get(k + 1)) else { break };
        let t = if cb - ca > 1e-15 { ((s - ca) / (cb - ca)).clamp(0.0, 1.0) } else { 0.0 };
        out.push(*a + (*b - *a) * t);
    }
    out
}

/// Rotation-minimising frames (double reflection) at the points, for the given unit tangents;
/// each station maps the first frame onto its own.
fn frames(pts: &[Vec3], tangents: &[Vec3]) -> Option<Vec<[Vec3; 3]>> {
    let t0 = *tangents.first()?;
    let helper = if t0.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let mut r = t0.cross(helper).normalized()?;
    let mut out = vec![[t0, r, t0.cross(r)]];
    for i in 0..pts.len().saturating_sub(1) {
        let (x0, x1, ti, tn) = (*pts.get(i)?, *pts.get(i + 1)?, *tangents.get(i)?, *tangents.get(i + 1)?);
        let v1 = x1 - x0;
        let c1 = v1.dot(v1);
        let (rl, tl) = if c1 > 1e-24 { (r - v1 * (2.0 / c1 * v1.dot(r)), ti - v1 * (2.0 / c1 * v1.dot(ti))) } else { (r, ti) };
        let v2 = tn - tl;
        let c2 = v2.dot(v2);
        r = if c2 > 1e-24 { rl - v2 * (2.0 / c2 * v2.dot(rl)) } else { rl };
        // Keep it exactly perpendicular and unit.
        r = (r - tn * r.dot(tn)).normalized()?;
        out.push([tn, r, tn.cross(r)]);
    }
    Some(out)
}

/// Stations: each maps a world vector at the first station to the same vector at its own,
/// optionally stretched across a corner (mitre).
fn stations(pts: &[Vec3], tangents: &[Vec3], mitre: &[Option<(Vec3, f64)>]) -> Result<Vec<Station>> {
    let f = frames(pts, tangents).ok_or_else(|| KernelError::Invalid("the sweep path doubles back".into()))?;
    let f0 = *f.first().ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
    let mut out = Vec::with_capacity(f.len());
    for (i, fi) in f.iter().enumerate() {
        // Rotation taking f0 to fi: columns are the images of the world axes.
        let col = |e: Vec3| fi[0] * f0[0].dot(e) + fi[1] * f0[1].dot(e) + fi[2] * f0[2].dot(e);
        let mut m = [col(Vec3::X), col(Vec3::Y), col(Vec3::Z)];
        if let Some(Some((b, s))) = mitre.get(i) {
            for c in &mut m {
                *c = *c + *b * ((s - 1.0) * c.dot(*b));
            }
        }
        out.push(Station { at: *pts.get(i).ok_or_else(|| KernelError::Invalid("sweep path".into()))?, m });
    }
    Ok(out)
}

/// The profile plane carried to a station.
fn station_plane(plane: &Plane, s0: Vec3, st: &Station) -> Plane {
    Plane { origin: st.at + st.apply(plane.origin - s0), x: st.apply(plane.x), y: st.apply(plane.y) }
}

/// Sweep a planar region along a 3D polyline path. The profile keeps its relation to the path
/// (it need not touch it); the path should start near the profile.
pub fn sweep_path(plane: &Plane, region: &Region2, path: &[Vec3]) -> Result<Body> {
    if region.area().abs() < 1e-9 {
        return Err(KernelError::Invalid("profile has no area".into()));
    }
    let pts = clean(path)?;
    let corner = pts.windows(3).any(|w| turn(w[1] - w[0], w[2] - w[1]) > CORNER);
    let lp = region.outer.ccw();
    if !corner && region.holes.is_empty() {
        // Smooth: evenly spaced stations, tangents from neighbours.
        let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
        let n = (pts.len() - 1).clamp(4, MAX_SMOOTH_STATIONS);
        let sp = resample(&pts, n);
        let tangents = central_tangents(&sp)?;
        let st = stations(&sp, &tangents, &[])?;
        let expected = region.area().abs() * len * start_cos(plane, &tangents);
        if let Ok(b) = smooth_walls(plane, &lp, &st, expected) {
            return Ok(b);
        }
    }
    ruled(plane, region, &pts)
}

fn central_tangents(p: &[Vec3]) -> Result<Vec<Vec3>> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let a = p.get(i.saturating_sub(1)).copied().unwrap_or_default();
            let b = p.get((i + 1).min(n - 1)).copied().unwrap_or_default();
            (b - a).normalized().ok_or_else(|| KernelError::Invalid("the sweep path has no direction".into()))
        })
        .collect()
}

/// How squarely the path leaves the profile (volume = area × length × this).
fn start_cos(plane: &Plane, tangents: &[Vec3]) -> f64 {
    tangents.first().map(|t| t.dot(plane.normal()).abs()).unwrap_or(1.0).max(1e-3)
}

/// Ruled walls between stations at the path's points (mitred at corners), every loop.
fn ruled(plane: &Plane, region: &Region2, pts: &[Vec3]) -> Result<Body> {
    let pts: Vec<Vec3> = if pts.len() > MAX_RULED_STATIONS { resample(pts, MAX_RULED_STATIONS) } else { pts.to_vec() };
    let n = pts.len();
    let seg_dir = |i: usize| -> Option<Vec3> { (*pts.get(i + 1)? - *pts.get(i)?).normalized() };
    let mut tangents = Vec::with_capacity(n);
    let mut mitre = Vec::with_capacity(n);
    for i in 0..n {
        let d_in = if i > 0 { seg_dir(i - 1) } else { None };
        let d_out = if i + 1 < n { seg_dir(i) } else { None };
        let (t, m) = match (d_in, d_out) {
            (Some(a), Some(b)) => {
                let t = (a + b).normalized().ok_or_else(|| KernelError::Invalid("the sweep path turns back on itself".into()))?;
                let half = turn(a, b) / 2.0;
                // Across the bend the section stretches by 1 / cos(half the turn).
                let m = (b - a).normalized().map(|bend| (bend, 1.0 / half.cos().max(0.2)));
                (t, m)
            }
            (Some(a), None) => (a, None),
            (None, Some(b)) => (b, None),
            (None, None) => return Err(KernelError::Invalid("sweep path".into())),
        };
        tangents.push(t);
        mitre.push(m);
    }
    let st = stations(&pts, &tangents, &mitre)?;
    let s0 = pts.first().copied().unwrap_or_default();
    let loops: Vec<Loop2> = std::iter::once(region.outer.ccw()).chain(region.holes.iter().map(|h| h.ccw().reversed())).collect();
    let mut wires: Vec<Vec<mt::Wire>> = Vec::with_capacity(st.len());
    for s in &st {
        let pl = station_plane(plane, s0, s);
        wires.push(loops.iter().map(|l| uniform_wire(&pl, l)).collect::<Result<Vec<_>>>()?);
    }
    guard("path sweep", || {
        let mut faces: Vec<mt::Face> = Vec::new();
        for w in wires.windows(2) {
            for (a, b) in w[0].iter().zip(&w[1]) {
                let sides = builder::try_wire_homotopy(a, b).map_err(|e| KernelError::Failed(format!("path sweep: {e}")))?;
                faces.extend(sides.face_iter().cloned());
            }
        }
        let (Some(first), Some(last)) = (wires.first(), wires.last()) else { return Err(KernelError::Failed("path sweep".into())) };
        cap_and_close(faces, first, last)
    })
}

/// One B-spline wall per profile segment through the stations, capped; checked by volume.
fn smooth_walls(plane: &Plane, lp: &Loop2, st: &[Station], expected: f64) -> Result<Body> {
    let nseg = lp.segs.len();
    let fail = |w: &str| KernelError::Failed(format!("path sweep: {w}"));
    if nseg == 0 || nseg > 64 || st.len() < 4 {
        return Err(fail("profile"));
    }
    let s0 = st.first().map(|s| s.at).unwrap_or_default();
    let at = |i: usize, q: Vec2| st.get(i).map(|s| s.at + s.apply(plane.to_world(q) - s0)).unwrap_or_default();
    let n = st.len() - 1;
    let mut data = Vec::with_capacity(nseg);
    for s in &lp.segs {
        let mut station_curves = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let pts: Vec<Vec3> = (0..SEG_SAMPLES).map(|j| at(i, s.point_at(j as f64 / (SEG_SAMPLES - 1) as f64))).collect();
            station_curves.push(interpolate_cubic(&pts).ok_or_else(|| fail("profile curve"))?);
        }
        let kv = station_curves.first().map(|c| c.knot_vec().clone()).ok_or_else(|| fail("no stations"))?;
        let mut columns = Vec::with_capacity(SEG_SAMPLES);
        for j in 0..SEG_SAMPLES {
            let pts: Vec<Vec3> = station_curves
                .iter()
                .map(|c| {
                    let p = c.control_point(j);
                    Vec3::new(p.x, p.y, p.z)
                })
                .collect();
            columns.push(interpolate_cubic(&pts).ok_or_else(|| fail("path curve"))?);
        }
        let ku = columns.first().map(|c| c.knot_vec().clone()).ok_or_else(|| fail("no columns"))?;
        let net: Vec<Vec<mt::Point3>> = (0..=n).map(|m| columns.iter().map(|c| *c.control_point(m)).collect()).collect();
        let surf = mt::BSplineSurface::try_new((ku, kv), net).map_err(|e| fail(&e.to_string()))?;
        let (Some(first), Some(last)) = (station_curves.first().cloned(), station_curves.last().cloned()) else { return Err(fail("stations")) };
        let seam = columns.first().cloned().ok_or_else(|| fail("seam"))?;
        data.push((first, last, seam, surf));
    }
    guard("path sweep", || {
        let start: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(at(0, s.start())))).collect();
        let end: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(at(n, s.start())))).collect();
        let (mut e0, mut e1, mut seams) = (Vec::with_capacity(nseg), Vec::with_capacity(nseg), Vec::with_capacity(nseg));
        for (j, (c0, c1, seam, _)) in data.iter().enumerate() {
            let (Some(a0), Some(b0), Some(a1), Some(b1)) = (start.get(j), start.get((j + 1) % nseg), end.get(j), end.get((j + 1) % nseg)) else {
                return Err(fail("vertices"));
            };
            e0.push(mt::Edge::try_new(a0, b0, mt::Curve::BSplineCurve(c0.clone())).map_err(|e| fail(&e.to_string()))?);
            e1.push(mt::Edge::try_new(a1, b1, mt::Curve::BSplineCurve(c1.clone())).map_err(|e| fail(&e.to_string()))?);
            seams.push(mt::Edge::try_new(a0, a1, mt::Curve::BSplineCurve(seam.clone())).map_err(|e| fail(&e.to_string()))?);
        }
        let mut walls = Vec::with_capacity(nseg);
        for (j, (_, _, _, surf)) in data.iter().enumerate() {
            let (Some(a), Some(b), Some(sa), Some(sb)) = (e0.get(j), e1.get(j), seams.get(j), seams.get((j + 1) % nseg)) else {
                return Err(fail("edges"));
            };
            let w: mt::Wire = vec![sa.clone(), b.clone(), sb.inverse(), a.inverse()].into();
            walls.push(mt::Face::try_new(vec![w], mt::Surface::BSplineSurface(surf.clone())).map_err(|e| fail(&e.to_string()))?);
        }
        let w0: mt::Wire = e0.iter().cloned().collect();
        let w1: mt::Wire = e1.iter().cloned().collect();
        let tol = (lp.signed_area().abs().sqrt() * 0.05).max(1e-3);
        for ws in [walls.clone(), walls.iter().map(|f| f.inverse()).collect()] {
            if let Ok(b) = cap_and_close(ws, std::slice::from_ref(&w0), std::slice::from_ref(&w1))
                && b.tessellate(tol).is_ok_and(|m| (m.measure().volume - expected).abs() <= 0.15 * expected.abs())
            {
                return Ok(b);
            }
        }
        Err(fail("the walls do not close a solid"))
    })
}

#[cfg(test)]
mod tests {
    use solvecraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

    use super::sweep_path;

    fn circle(r: f64) -> Region2 {
        Region2 { outer: Loop2::circle(Vec2::ZERO, r), holes: Vec::new() }
    }

    #[test]
    fn sweeps_along_3d_paths() {
        // A quarter of a helix-like 3D curve: a smooth wall, volume ≈ area × length.
        let path: Vec<Vec3> = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0 * std::f64::consts::FRAC_PI_2;
                Vec3::new(30.0 * t.sin(), 30.0 * (1.0 - t.cos()), 10.0 * t)
            })
            .collect();
        let t0 = (path[1] - path[0]).normalized().unwrap();
        let plane = Plane::from_normal(path[0], t0).unwrap();
        let b = sweep_path(&plane, &circle(2.0), &path).unwrap();
        let len: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
        let v = b.tessellate(0.01).unwrap().measure().volume;
        let want = std::f64::consts::PI * 4.0 * len;
        assert!((v - want).abs() < 0.02 * want, "{v} vs {want}");
        // One wall per profile segment and two caps (ruled walls would give dozens).
        assert!(b.faces(0.01).unwrap().len() <= 8, "smooth walls: {}", b.faces(0.01).unwrap().len());
        // An L-shaped path (a sharp corner) with a holed square: mitred ruled walls.
        let path = [Vec3::ZERO, Vec3::new(0.0, 0.0, 20.0), Vec3::new(20.0, 0.0, 20.0)];
        let plane = Plane::from_normal(Vec3::ZERO, Vec3::Z).unwrap();
        let sq = |h: f64| Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]);
        let region = Region2 { outer: sq(3.0), holes: vec![sq(1.0).reversed()] };
        let b = sweep_path(&plane, &region, &path).unwrap();
        let v = b.tessellate(0.01).unwrap().measure().volume;
        let want = 32.0 * 40.0;
        assert!((v - want).abs() < 0.03 * want, "{v} vs {want}");
        assert!(sweep_path(&plane, &region, &[Vec3::ZERO]).is_err());
        assert!(sweep_path(&plane, &region, &[Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
    }
}
