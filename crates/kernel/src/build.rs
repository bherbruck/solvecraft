//! Building bodies: extrude and revolve planar regions, primitives.

use mt::builder;
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, Solid, p3, v3};
use crate::{KernelError, Result, guard};

const MAX_SEGS: usize = 20_000;

fn finite(x: f64, what: &str) -> Result<f64> {
    if x.is_finite() && x.abs() < 1e7 { Ok(x) } else { Err(KernelError::Invalid(format!("{what} = {x}"))) }
}

/// A closed wire in 3D from a sketch loop.
pub(crate) fn wire(plane: &Plane, lp: &Loop2) -> Result<mt::Wire> {
    let n = lp.segs.len();
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid(format!("loop with {n} segments")));
    }
    if !lp.is_closed(1e-6) {
        return Err(KernelError::Invalid("profile loop is not closed".into()));
    }
    let verts: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
    let mut edges = Vec::with_capacity(n);
    for (i, s) in lp.segs.iter().enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % n)) else { continue };
        let e: mt::Edge = match *s {
            Seg2::Line { .. } => builder::line(a, b),
            Seg2::Arc { sweep, .. } => {
                if sweep.abs() < 1e-9 {
                    return Err(KernelError::Invalid("zero-length arc".into()));
                }
                // Split big arcs so every piece is well conditioned.
                if sweep.abs() > std::f64::consts::PI * 1.5 {
                    let vm = builder::vertex(p3(plane.to_world(s.mid())));
                    edges.push(builder::circle_arc(a, &vm, p3(plane.to_world(s.point_at(0.25)))));
                    builder::circle_arc(&vm, b, p3(plane.to_world(s.point_at(0.75))))
                } else {
                    builder::circle_arc(a, b, p3(plane.to_world(s.mid())))
                }
            }
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
                crate::freeform::edge(plane, s, a, b).ok_or_else(|| KernelError::Invalid("profile curve".into()))?
            }
        };
        edges.push(e);
    }
    Ok(edges.into())
}

/// A planar face from a region (outer loop with holes).
pub(crate) fn face(plane: &Plane, r: &Region2) -> Result<mt::Face> {
    let mut wires = vec![wire(plane, &r.outer.ccw())?];
    for h in &r.holes {
        wires.push(wire(plane, &h.ccw().reversed())?);
    }
    guard("face", || builder::try_attach_plane(&wires).map_err(|e| KernelError::Invalid(format!("profile: {e}"))))
}

fn region_ok(r: &Region2) -> Result<()> {
    if r.area().abs() < 1e-9 {
        return Err(KernelError::Invalid("profile has no area".into()));
    }
    for s in std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs) {
        let ok = match *s {
            Seg2::Line { a, b } => a.is_finite() && b.is_finite(),
            Seg2::Arc { center, radius, start, sweep } => {
                center.is_finite() && radius.is_finite() && radius > 1e-9 && start.is_finite() && sweep.is_finite()
            }
            Seg2::Cubic { p0, p1, p2, p3 } => p0.is_finite() && p1.is_finite() && p2.is_finite() && p3.is_finite(),
            Seg2::Conic { a, apex, b, w } => a.is_finite() && apex.is_finite() && b.is_finite() && w.is_finite() && w > 1e-9,
            Seg2::Bezier { n, p } => (4..=solvecraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) && p.iter().all(|q| q.is_finite()),
        };
        if !ok {
            return Err(KernelError::Invalid("non-finite profile".into()));
        }
    }
    Ok(())
}

/// Extrude each region from `start` to `end` along the plane normal (offsets in mm, `end > start`).
pub fn extrude(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    let (start, end) = (finite(start, "start")?, finite(end, "end")?);
    let (lo, hi) = if end >= start { (start, end) } else { (end, start) };
    if hi - lo < 1e-6 {
        return Err(KernelError::Invalid("extrude distance is zero".into()));
    }
    if regions.is_empty() {
        return Err(KernelError::Invalid("no profile selected".into()));
    }
    let n = plane.normal();
    let mut out = Vec::new();
    for r in regions {
        region_ok(r)?;
        let base = plane.offset(lo);
        let f = face(&base, r)?;
        let solid: Solid = guard("extrude", || Ok(builder::tsweep(&f, v3(n * (hi - lo)))))?;
        out.push(Body::new(solid)?);
    }
    Ok(out)
}

/// Revolve each region about the axis through sketch point `axis_origin` with sketch direction
/// `axis_dir` by `angle` radians (|angle| ≥ 2π = full revolution).
pub fn revolve(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    let angle = finite(angle, "angle")?;
    if angle.abs() < 1e-6 {
        return Err(KernelError::Invalid("revolve angle is zero".into()));
    }
    let d = axis_dir.normalized().ok_or_else(|| KernelError::Invalid("revolve axis has no direction".into()))?;
    if regions.is_empty() {
        return Err(KernelError::Invalid("no profile selected".into()));
    }
    // The profile must lie on one side of the axis.
    let o = plane.to_world(axis_origin);
    let axis = plane.dir_to_world(d);
    let mut out = Vec::new();
    for r in regions {
        region_ok(r)?;
        let (mut neg, mut pos) = (false, false);
        for p in r.outer.polyline(1e-3) {
            let s = d.cross(p - axis_origin);
            neg |= s < -1e-7;
            pos |= s > 1e-7;
        }
        if neg && pos {
            return Err(KernelError::Invalid("the profile crosses the revolve axis".into()));
        }
        // Points on the axis (within solver tolerance) go exactly onto it, so the sweep does not
        // create hair-thin faces.
        let snap = |p: Vec2| {
            let along = (p - axis_origin).dot(d);
            let q = axis_origin + d * along;
            if p.dist(q) < 1e-7 { q } else { p }
        };
        let snap_loop = |l: &Loop2| Loop2 {
            segs: l
                .segs
                .iter()
                .map(|s| match *s {
                    Seg2::Line { a, b } => Seg2::Line { a: snap(a), b: snap(b) },
                    arc => arc,
                })
                .collect(),
        };
        // Full circles start at an odd angle: their seam points then don't lie in the planes
        // through the profile's centre that other bodies tend to cut along.
        let reseam = |l: Loop2| -> Loop2 {
            let c = match l.segs.first() {
                Some(Seg2::Arc { center, radius, .. }) => Some((*center, *radius)),
                _ => None,
            };
            // (A circle touching the axis keeps its vertex there.)
            let touches = c.is_some_and(|(center, radius)| (d.cross(center - axis_origin).abs() - radius).abs() < 1e-7);
            match c {
                Some((center, radius))
                    if !touches
                        && l.segs.iter().all(
                            |s| matches!(s, Seg2::Arc { center: c2, radius: r2, .. } if c2.dist(center) < 1e-9 && (r2 - radius).abs() < 1e-9),
                        )
                        && (l
                            .segs
                            .iter()
                            .map(|s| match s {
                                Seg2::Arc { sweep, .. } => *sweep,
                                _ => 0.0,
                            })
                            .sum::<f64>()
                            .abs()
                            - std::f64::consts::TAU)
                            .abs()
                            < 1e-6 =>
                {
                    let ccw = l.signed_area() > 0.0;
                    let n = Loop2::circle_from(center, radius, 0.4142);
                    if ccw { n } else { n.reversed() }
                }
                _ => l,
            }
        };
        let r = &Region2 { outer: reseam(snap_loop(&r.outer)), holes: r.holes.iter().map(|h| reseam(snap_loop(h))).collect() };
        // A half disc on the axis revolved fully is a sphere: build it without poles.
        if angle.abs() >= std::f64::consts::TAU - 1e-9
            && r.holes.is_empty()
            && let Some((c, rad)) = half_disc(&r.outer, axis_origin, d)
        {
            out.push(sphere_patches(plane.to_world(c), rad)?);
            continue;
        }
        // A full revolution of a profile with one edge on the axis: sweep the rest of the loop
        // as a cone-like shell (no zero-area faces, which booleans can't handle).
        if angle.abs() >= std::f64::consts::TAU - 1e-9
            && r.holes.is_empty()
            && let Some(b) = revolve_touching_axis(plane, &r.outer, axis_origin, d)?
        {
            out.push(b);
            continue;
        }
        let f = face(plane, r)?;
        // Partial revolves sweep toward the side the sketch normal points to (as features
        // extrude toward it).
        let c = plane.to_world(r.centroid());
        let radial = (c - o) - axis * (c - o).dot(axis);
        let toward = axis.cross(radial).dot(plane.normal());
        let angle = if toward < 0.0 { -angle } else { angle };
        let solid: Solid = guard("revolve", || Ok(builder::rsweep(&f, p3(o), v3(axis), mt::Rad(angle))))?;
        out.push(Body::new(solid)?);
    }
    Ok(out)
}

/// Axis-aligned box between two corners.
pub fn box_solid(a: Vec3, b: Vec3) -> Result<Body> {
    let (lo, hi) = (a.min(b), a.max(b));
    let s = hi - lo;
    if !(s.x > 1e-6 && s.y > 1e-6 && s.z > 1e-6 && s.is_finite() && s.len() < 1e7) {
        return Err(KernelError::Invalid("box needs a positive size in x, y and z".into()));
    }
    let r = Region2 {
        outer: Loop2::polygon(&[Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y), Vec2::new(lo.x, hi.y)]),
        holes: vec![],
    };
    let mut v = extrude(&Plane::XY, &[r], lo.z, hi.z)?;
    v.pop().ok_or_else(|| KernelError::Failed("box".into()))
}

/// Cylinder with base centre `base`, axis direction `axis`, radius and height.
pub fn cylinder(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Result<Body> {
    let plane = Plane::from_normal(base, axis).ok_or_else(|| KernelError::Invalid("cylinder axis".into()))?;
    let radius = finite(radius, "radius")?;
    if radius <= 1e-6 {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    let r = Region2 { outer: Loop2::circle(Vec2::ZERO, radius), holes: vec![] };
    let mut v = extrude(&plane, &[r], 0.0, finite(height, "height")?)?;
    v.pop().ok_or_else(|| KernelError::Failed("cylinder".into()))
}

/// Sphere: revolve a half disc.
pub fn sphere(center: Vec3, radius: f64) -> Result<Body> {
    sphere_patches(center, radius)
}

/// Torus about the Z axis through `center`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
    let (major, minor) = (finite(major, "major radius")?, finite(minor, "minor radius")?);
    if !(minor > 1e-6 && major > minor) {
        return Err(KernelError::Invalid("torus needs major > minor > 0".into()));
    }
    let plane = Plane { origin: center, ..Plane::XZ };
    let r = Region2 { outer: Loop2::circle(Vec2::new(major, 0.0), minor), holes: vec![] };
    let mut v = revolve(&plane, &[r], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?;
    v.pop().ok_or_else(|| KernelError::Failed("torus".into()))
}

/// Offset every segment of a loop to its right by `d` (outward for a region's outer loop and
/// holes alike). Loops of lines (mitred corners) and loops of arcs on one circle are supported.
fn offset_loop(lp: &Loop2, d: f64) -> Result<Loop2> {
    if d == 0.0 {
        return Ok(lp.clone());
    }
    let all_arcs = lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. }));
    if all_arcs {
        let segs = lp
            .segs
            .iter()
            .map(|s| match *s {
                Seg2::Arc { center, radius, start, sweep } => {
                    let r = radius + sweep.signum() * d;
                    if r > 1e-6 {
                        Ok(Seg2::Arc { center, radius: r, start, sweep })
                    } else {
                        Err(KernelError::Invalid("the taper closes the profile".into()))
                    }
                }
                _ => Err(KernelError::Failed("not supported yet: tapered profiles mixing lines and arcs".into())),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(Loop2 { segs });
    }
    let n = lp.segs.len();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::with_capacity(n);
    for s in &lp.segs {
        let Seg2::Line { a, b } = *s else { return Err(KernelError::Failed("not supported yet: tapered profiles mixing lines and arcs".into())) };
        let dir = (b - a).normalized().ok_or_else(|| KernelError::Invalid("zero-length edge".into()))?;
        let right = Vec2::new(dir.y, -dir.x);
        lines.push((a + right * d, dir));
    }
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(&(p0, d0)), Some(&(p1, d1))) = (lines.get((i + n - 1) % n), lines.get(i)) else { continue };
        let den = d0.cross(d1);
        if den.abs() < 1e-12 {
            pts.push(p1);
            continue;
        }
        let t = (p1 - p0).cross(d1) / den;
        pts.push(p0 + d0 * t);
    }
    let out = Loop2::polygon(&pts);
    if out.signed_area().signum() != lp.signed_area().signum() || out.signed_area().abs() < 1e-9 {
        return Err(KernelError::Invalid("the taper closes the profile".into()));
    }
    Ok(out)
}

/// Extrude one region by `length` along `dir_sign` × the plane normal with a taper angle
/// (radians; positive grows the profile).
pub fn extrude_tapered(plane: &Plane, region: &Region2, length: f64, dir_sign: f64, taper: f64) -> Result<Body> {
    let (length, taper) = (finite(length, "distance")?, finite(taper, "taper")?);
    if length <= 1e-6 {
        return Err(KernelError::Invalid("extrude distance is zero".into()));
    }
    if taper.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("taper must be between −90° and 90°".into()));
    }
    region_ok(region)?;
    let d = length * taper.tan();
    let top_plane = plane.offset(length * dir_sign.signum());
    let loops0: Vec<Loop2> = std::iter::once(region.outer.ccw()).chain(region.holes.iter().map(|h| h.ccw().reversed())).collect();
    let loops1: Vec<Loop2> = loops0.iter().map(|l| offset_loop(l, d)).collect::<Result<_>>()?;
    guard("tapered extrude", || {
        let w0: Vec<mt::Wire> = loops0.iter().map(|l| wire(plane, l)).collect::<Result<_>>()?;
        let w1: Vec<mt::Wire> = loops1.iter().map(|l| wire(&top_plane, l)).collect::<Result<_>>()?;
        let mut faces: Vec<mt::Face> = Vec::new();
        for (a, b) in w0.iter().zip(&w1) {
            let sides = builder::try_wire_homotopy(a, b).map_err(|e| KernelError::Failed(format!("taper sides: {e}")))?;
            faces.extend(sides.face_iter().cloned());
        }
        let bottom = builder::try_attach_plane(&w0.iter().map(|w| w.inverse()).collect::<Vec<_>>())
            .map_err(|e| KernelError::Failed(format!("taper bottom: {e}")))?;
        let top = builder::try_attach_plane(&w1).map_err(|e| KernelError::Failed(format!("taper top: {e}")))?;
        let mut tries: Vec<(mt::Face, mt::Face)> = vec![(bottom.clone(), top.clone())];
        tries.push((bottom.inverse(), top.inverse()));
        let mut last = String::new();
        for (b, t) in tries {
            let mut all = faces.clone();
            all.push(b);
            all.push(t);
            let shell: mt::Shell = all.into();
            match Solid::try_new(vec![shell]) {
                Ok(s) => return Body::new(s),
                Err(e) => last = e.to_string(),
            }
        }
        Err(KernelError::Failed(format!("tapered extrude: {last}")))
    })
}

/// Revolve 360° a loop that has exactly one line segment on the axis, without degenerate faces.
/// A loop made of a half circle and its diameter on the axis: (centre, radius).
fn half_disc(lp: &Loop2, axis_origin: Vec2, d: Vec2) -> Option<(Vec2, f64)> {
    let on_axis = |p: Vec2| d.cross(p - axis_origin).abs() < 1e-7 * (1.0 + p.len());
    let mut arcs = Vec::new();
    for s in &lp.segs {
        match *s {
            Seg2::Line { a, b } if on_axis(a) && on_axis(b) => {}
            Seg2::Arc { center, radius, sweep, .. } if on_axis(center) => arcs.push((center, radius, sweep)),
            _ => return None,
        }
    }
    let (c, r, _) = *arcs.first()?;
    let total: f64 = arcs.iter().map(|a| a.2.abs()).sum();
    let same = arcs.iter().all(|(c2, r2, _)| c2.dist(c) < 1e-7 * (1.0 + r) && (r2 - r).abs() < 1e-7 * (1.0 + r));
    (same && (total - std::f64::consts::PI).abs() < 1e-6).then_some((c, r))
}

fn revolve_touching_axis(plane: &Plane, lp: &Loop2, axis_origin: Vec2, d: Vec2) -> Result<Option<Body>> {
    let on_axis = |p: Vec2| d.cross(p - axis_origin).abs() < 1e-9;
    let n = lp.segs.len();
    let axis_segs: Vec<usize> =
        (0..n).filter(|i| lp.segs.get(*i).is_some_and(|s| matches!(s, Seg2::Line { a, b } if on_axis(*a) && on_axis(*b)))).collect();
    let [k] = axis_segs[..] else { return Ok(None) };
    // The open chain after the axis segment, back around to it.
    let mut chain: Vec<Seg2> = (1..n).filter_map(|j| lp.segs.get((k + j) % n).copied()).collect();
    // A single arc (a sphere) is split off-centre: a seam on the equator would lie in the very
    // planes other bodies are often cut with.
    if let [Seg2::Arc { center, radius, start, sweep }] = chain[..] {
        let f = 0.37;
        chain = vec![
            Seg2::Arc { center, radius, start, sweep: sweep * f },
            Seg2::Arc { center, radius, start: start + sweep * f, sweep: sweep * (1.0 - f) },
        ];
    }
    if chain.is_empty() || chain.iter().skip(1).take(chain.len().saturating_sub(2)).any(|s| on_axis(s.start()) && on_axis(s.end())) {
        return Ok(None);
    }
    // Built with the axis through the world origin (truck tessellates such cones reliably), then
    // moved into place.
    let shift = plane.to_world(axis_origin);
    let local = Plane { origin: plane.origin - shift, ..*plane };
    let plane = &local;
    // Radial lines at the ends become planar caps (no degenerate disc centres).
    let radial = |sg: &Seg2| matches!(*sg, Seg2::Line { a, b } if (b - a).normalized().is_some_and(|u| u.dot(d).abs() < 1e-9));
    let mut mid: &[Seg2] = &chain;
    if mid.first().is_some_and(|f| radial(f) && on_axis(f.start())) {
        mid = mid.get(1..).unwrap_or(&[]);
    }
    if mid.last().is_some_and(|l| radial(l) && on_axis(l.end())) {
        mid = mid.get(..mid.len().saturating_sub(1)).unwrap_or(&[]);
    }
    if mid.is_empty() {
        return Ok(None);
    }
    let apex_start = mid.first().is_some_and(|f| on_axis(f.start()));
    let apex_end = mid.last().is_some_and(|l| on_axis(l.end()));
    guard("revolve", || {
        let verts: Vec<mt::Vertex> = mid
            .iter()
            .map(|s| builder::vertex(p3(plane.to_world(s.start()))))
            .chain(mid.last().map(|s| builder::vertex(p3(plane.to_world(s.end())))))
            .collect();
        let mut edges: Vec<mt::Edge> = Vec::new();
        for (i, s) in mid.iter().enumerate() {
            let (Some(a), Some(b)) = (verts.get(i), verts.get(i + 1)) else { continue };
            edges.push(match *s {
                Seg2::Line { .. } => builder::line(a, b),
                Seg2::Arc { .. } => builder::circle_arc(a, b, p3(plane.to_world(s.mid()))),
                Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => match crate::freeform::edge(plane, s, a, b) {
                    Some(e) => e,
                    None => builder::line(a, b),
                },
            });
        }
        let mut w: mt::Wire = edges.into();
        let axis = plane.dir_to_world(d);
        let mut shell = if apex_start || apex_end {
            if !apex_start {
                // builder::cone wants the apex first.
                w = w.inverse();
            }
            builder::cone(&w, v3(axis), mt::Rad(std::f64::consts::TAU))
        } else {
            builder::rsweep(&w, p3(plane.to_world(axis_origin)), v3(axis), mt::Rad(std::f64::consts::TAU))
        };
        let sides = shell.len();
        for b in shell.extract_boundaries() {
            let cap = builder::try_attach_plane(&[b]).map_err(|e| KernelError::Failed(format!("revolve cap: {e}")))?;
            shell.push(cap);
        }
        let solid = Solid::try_new(vec![shell.clone()]).or_else(|_| {
            // Caps may come out facing inwards: flip them.
            let mut s2 = shell.clone();
            for f in s2.face_iter_mut().skip(sides) {
                f.invert();
            }
            Solid::try_new(vec![s2])
        });
        let solid = solid.map_err(|e| KernelError::Failed(format!("revolve: {e}")))?;
        let solid = if shift.len() > 0.0 { builder::translated(&solid, v3(shift)) } else { solid };
        Body::new(solid).map(Some)
    })
}

/// Close a ruled shell (side faces between matching wires) with planar caps into a body.
pub(crate) fn cap_and_close(mut faces: Vec<mt::Face>, w0: &[mt::Wire], w1: &[mt::Wire]) -> Result<Body> {
    let bottom =
        builder::try_attach_plane(&w0.iter().map(|w| w.inverse()).collect::<Vec<_>>()).map_err(|e| KernelError::Failed(format!("cap: {e}")))?;
    let top = builder::try_attach_plane(w1).map_err(|e| KernelError::Failed(format!("cap: {e}")))?;
    let mut last = String::new();
    for (b, t) in [(bottom.clone(), top.clone()), (bottom.inverse(), top.inverse()), (bottom.clone(), top.inverse()), (bottom.inverse(), top)] {
        let mut all = std::mem::take(&mut faces);
        let keep = all.clone();
        all.push(b);
        all.push(t);
        let shell: mt::Shell = all.into();
        match Solid::try_new(vec![shell]) {
            Ok(s) => return Body::new(s),
            Err(e) => last = e.to_string(),
        }
        faces = keep;
    }
    Err(KernelError::Failed(format!("closing the shell: {last}")))
}

/// Re-split a loop into exactly `n` segments: a full circle gets breaks at `angles` (or evenly),
/// other loops split their longest segments in half.
fn resample(lp: &Loop2, n: usize, angles: &[f64]) -> Loop2 {
    let circle = lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. }))
        && lp.segs.windows(2).all(|w| match (w[0], w[1]) {
            (Seg2::Arc { center: c1, radius: r1, .. }, Seg2::Arc { center: c2, radius: r2, .. }) => c1.dist(c2) < 1e-9 && (r1 - r2).abs() < 1e-9,
            _ => false,
        });
    if circle && let Some(Seg2::Arc { center, radius, .. }) = lp.segs.first().copied() {
        let mut a: Vec<f64> = if angles.len() == n { angles.to_vec() } else { (0..n).map(|i| std::f64::consts::TAU * i as f64 / n as f64).collect() };
        // Keep the given order but make them increase.
        for i in 1..a.len() {
            while a[i] <= a[i - 1] {
                a[i] += std::f64::consts::TAU;
            }
        }
        let segs = (0..n)
            .map(|i| {
                let s = a[i];
                let e = if i + 1 < n { a[i + 1] } else { a[0] + std::f64::consts::TAU };
                Seg2::Arc { center, radius, start: s, sweep: e - s }
            })
            .collect();
        return Loop2 { segs };
    }
    let mut segs = lp.segs.clone();
    while segs.len() < n {
        let Some((k, _)) = segs.iter().enumerate().max_by(|a, b| a.1.length().total_cmp(&b.1.length())) else { break };
        let s = segs.remove(k);
        let (p, q) = s.split_half();
        segs.insert(k, q);
        segs.insert(k, p);
    }
    Loop2 { segs }
}

/// Ruled loft through two or more planar sections (outer loops only).
pub fn loft(sections: &[(Plane, Loop2)]) -> Result<Body> {
    if sections.len() < 2 || sections.len() > 50 {
        return Err(KernelError::Invalid("a loft needs 2…50 sections".into()));
    }
    let n = sections.iter().map(|(_, l)| l.segs.len()).max().unwrap_or(0);
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid("empty section".into()));
    }
    // Orient every section counter-clockwise about the first section's normal, then match
    // segment counts and starting points to the section with the most segments.
    let n0 = sections[0].0.normal();
    let oriented: Vec<(Plane, Loop2)> = sections
        .iter()
        .map(|(p, l)| {
            let l = if p.normal().dot(n0) >= 0.0 { l.ccw() } else { l.ccw().reversed() };
            (*p, l)
        })
        .collect();
    let lead = oriented.iter().max_by_key(|(_, l)| l.segs.len()).cloned().ok_or_else(|| KernelError::Invalid("no sections".into()))?;
    let lead_c = lead.0.to_world(lead.1.centroid());
    let lead_dirs: Vec<Vec3> = lead.1.segs.iter().map(|s| lead.0.to_world(s.start()) - lead_c).collect();
    let mut wires: Vec<mt::Wire> = Vec::new();
    for (p, l) in &oriented {
        let c = l.centroid();
        // Break angles: the lead section's vertex directions seen in this plane.
        let angles: Vec<f64> = lead_dirs.iter().map(|d| Vec2::new(d.dot(p.x), d.dot(p.y)).angle()).collect();
        let mut r = resample(l, n, &angles);
        // Start at the vertex closest in direction to the lead's first vertex.
        if let Some(d0) = lead_dirs.first() {
            let target = Vec2::new(d0.dot(p.x), d0.dot(p.y)).angle();
            let best = (0..r.segs.len()).min_by(|a, b| {
                let ang = |k: usize| {
                    let v = r.segs[k].start() - c;
                    let d = (v.angle() - target).rem_euclid(std::f64::consts::TAU);
                    d.min(std::f64::consts::TAU - d)
                };
                ang(*a).total_cmp(&ang(*b))
            });
            if let Some(k) = best {
                r.segs.rotate_left(k);
            }
        }
        wires.push(uniform_wire(p, &r)?);
    }
    guard("loft", || {
        let mut faces: Vec<mt::Face> = Vec::new();
        for w in wires.windows(2) {
            let sides = builder::try_wire_homotopy(&w[0], &w[1]).map_err(|e| KernelError::Failed(format!("loft: {e}")))?;
            faces.extend(sides.face_iter().cloned());
        }
        let (Some(first), Some(last)) = (wires.first(), wires.last()) else { return Err(KernelError::Failed("loft".into())) };
        cap_and_close(faces, std::slice::from_ref(first), std::slice::from_ref(last))
    })
}

/// A 3D path segment for sweeps.
#[derive(Clone, Copy, Debug)]
pub enum PathSeg {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// Arc around `center`, rotating by `angle` about `axis` (right hand) from `a`.
    Arc {
        a: Vec3,
        center: Vec3,
        axis: Vec3,
        angle: f64,
    },
}

/// Sweep a planar region (outer loop) along a chain of path segments, keeping the profile
/// perpendicular to the path (lines translate it, arcs rotate it).
pub fn sweep(plane: &Plane, region: &Region2, path: &[PathSeg]) -> Result<Body> {
    region_ok(region)?;
    if path.is_empty() || path.len() > 1000 {
        return Err(KernelError::Invalid("the path needs 1…1000 segments".into()));
    }
    // The outer loop and each hole, swept along the path; the ends are capped with holes.
    let mut starts = vec![wire(plane, &region.outer.ccw())?];
    for h in &region.holes {
        starts.push(wire(plane, &h.ccw().reversed())?);
    }
    guard("sweep", || {
        let mut shell = mt::Shell::new();
        let mut ends = Vec::new();
        for w0 in &starts {
            let mut cur = w0.clone();
            for seg in path {
                let mut part: mt::Shell = match *seg {
                    PathSeg::Line { a, b } => builder::tsweep(&cur, v3(b - a)),
                    PathSeg::Arc { center, axis, angle, .. } => {
                        let ax = axis.normalized().ok_or_else(|| KernelError::Invalid("arc axis".into()))?;
                        builder::rsweep(&cur, p3(center), v3(ax), mt::Rad(angle))
                    }
                };
                let next = part
                    .extract_boundaries()
                    .into_iter()
                    .find(|w| w.edge_iter().all(|e| !cur.edge_iter().any(|c| c.id() == e.id())))
                    .ok_or_else(|| KernelError::Failed("sweep: lost the profile".into()))?;
                shell.append(&mut part);
                cur = next.inverse();
            }
            ends.push(cur);
        }
        let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
        cap_and_close(faces, &starts, &ends)
    })
}

/// Cox–de Boor basis values N_{j,3}(u) for all j.
fn cubic_basis(knots: &[f64], n_ctrl: usize, u: f64) -> Vec<f64> {
    let p = 3;
    let m = knots.len();
    let mut n = vec![0.0; m.saturating_sub(1)];
    let last = n_ctrl.saturating_sub(1);
    for (j, v) in n.iter_mut().enumerate() {
        let (Some(&a), Some(&b)) = (knots.get(j), knots.get(j + 1)) else { continue };
        // Half-open spans, with the last span closed at the end.
        if (a <= u && u < b) || (u >= b && (b - a) > 0.0 && j == last && (u - b).abs() < 1e-12) {
            *v = 1.0;
        }
    }
    for k in 1..=p {
        for j in 0..m.saturating_sub(1 + k) {
            let (Some(&tj), Some(&tjk), Some(&tj1), Some(&tjk1)) = (knots.get(j), knots.get(j + k), knots.get(j + 1), knots.get(j + k + 1)) else {
                continue;
            };
            let left = if tjk - tj > 0.0 { (u - tj) / (tjk - tj) * n.get(j).copied().unwrap_or(0.0) } else { 0.0 };
            let right = if tjk1 - tj1 > 0.0 { (tjk1 - u) / (tjk1 - tj1) * n.get(j + 1).copied().unwrap_or(0.0) } else { 0.0 };
            if let Some(v) = n.get_mut(j) {
                *v = left + right;
            }
        }
    }
    n.truncate(n_ctrl);
    n
}

/// Cubic B-spline through points at uniform parameters (clamped, knots by averaging).
pub(crate) fn interpolate_cubic(pts: &[Vec3]) -> Option<mt::BSplineCurve<mt::Point3>> {
    let m = pts.len();
    if m < 4 {
        return None;
    }
    let u: Vec<f64> = (0..m).map(|i| i as f64 / (m - 1) as f64).collect();
    let mut knots = vec![0.0; 4];
    for j in 1..(m - 3) {
        knots.push((j..j + 3).filter_map(|i| u.get(i)).sum::<f64>() / 3.0);
    }
    knots.extend([1.0; 4]);
    // Dense solve of N · P = Q for each coordinate.
    let mut a: Vec<Vec<f64>> = u.iter().map(|ui| cubic_basis(&knots, m, *ui)).collect();
    let mut q: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    for col in 0..m {
        let piv = (col..m).max_by(|x, y| a[*x][col].abs().total_cmp(&a[*y][col].abs()))?;
        if a[piv][col].abs() < 1e-14 {
            return None;
        }
        a.swap(col, piv);
        q.swap(col, piv);
        for r in 0..m {
            if r == col {
                continue;
            }
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            let prow = a[col].clone();
            for (x, v) in a[r].iter_mut().zip(&prow).skip(col) {
                *x -= f * v;
            }
            let qc = q[col];
            for k in 0..3 {
                q[r][k] -= f * qc[k];
            }
        }
    }
    let ctrl: Vec<mt::Point3> = (0..m).map(|i| mt::Point3::new(q[i][0] / a[i][i], q[i][1] / a[i][i], q[i][2] / a[i][i])).collect();
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).ok()
}

/// A closed wire whose edges are cubic B-splines parameterised (nearly) by arc length, so that
/// ruled surfaces between sections pair points proportionally along each edge.
pub(crate) fn uniform_wire(plane: &Plane, lp: &Loop2) -> Result<mt::Wire> {
    const SAMPLES: usize = 24;
    let n = lp.segs.len();
    let verts: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
    let mut edges = Vec::with_capacity(n);
    for (i, s) in lp.segs.iter().enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % n)) else { continue };
        let pts: Vec<Vec3> = (0..SAMPLES).map(|k| plane.to_world(s.point_at(k as f64 / (SAMPLES - 1) as f64))).collect();
        let curve = interpolate_cubic(&pts).ok_or_else(|| KernelError::Failed("loft curve".into()))?;
        edges.push(mt::Edge::new(a, b, mt::Curve::BSplineCurve(curve)));
    }
    Ok(edges.into())
}

/// A sphere as six patches (the cube's faces projected onto it) on revolved surfaces whose poles
/// and seams lie outside each patch. No face has a singular point, which the boolean needs; the
/// patches are one analytic sphere, so measured topology counts it as one face.
pub fn sphere_patches(center: Vec3, radius: f64) -> Result<Body> {
    sphere_patches_rotated(center, radius, [Vec3::X, Vec3::Y, Vec3::Z])
}

/// [`sphere_patches`] with the patch layout turned to the frame `rot` (orthonormal columns).
pub fn sphere_patches_rotated(center: Vec3, radius: f64, rot: [Vec3; 3]) -> Result<Body> {
    let radius = finite(radius, "radius")?;
    let turn = |v: Vec3| rot[0] * v.x + rot[1] * v.y + rot[2] * v.z;
    if radius <= 1e-6 || !center.is_finite() {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    let k = radius / 3f64.sqrt();
    let corner = |sx: f64, sy: f64, sz: f64| center + turn(Vec3::new(sx * k, sy * k, sz * k));
    // Cube faces: outward direction, rotation axis for the patch's surface (perpendicular to it),
    // and the four corner sign triples counter-clockwise seen from outside.
    let faces: [(Vec3, Vec3, [[f64; 3]; 4]); 6] = [
        (Vec3::X, Vec3::Z, [[1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]]),
        (-Vec3::X, Vec3::Z, [[-1.0, 1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0]]),
        (Vec3::Y, Vec3::Z, [[1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]]),
        (-Vec3::Y, Vec3::Z, [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]]),
        (Vec3::Z, Vec3::X, [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
        (-Vec3::Z, Vec3::X, [[-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, -1.0, -1.0], [-1.0, -1.0, -1.0]]),
    ];
    guard("sphere", || {
        let key = |s: [f64; 3]| ((s[0] > 0.0) as usize) | (((s[1] > 0.0) as usize) << 1) | (((s[2] > 0.0) as usize) << 2);
        let mut verts: std::collections::HashMap<usize, mt::Vertex> = std::collections::HashMap::new();
        let mut vert = |s: [f64; 3]| verts.entry(key(s)).or_insert_with(|| builder::vertex(p3(corner(s[0], s[1], s[2])))).clone();
        let mut edges: std::collections::HashMap<(usize, usize), mt::Edge> = std::collections::HashMap::new();
        let mut out: Vec<mt::Face> = Vec::new();
        for (dir, axis, cs) in faces {
            let (dir, axis) = (turn(dir), turn(axis));
            let mut wire: Vec<mt::Edge> = Vec::new();
            for i in 0..4 {
                let (a, b) = (cs[i], cs[(i + 1) % 4]);
                let (ka, kb) = (key(a), key(b));
                let e = match edges.get(&(ka.min(kb), ka.max(kb))) {
                    Some(e) => e.clone(),
                    None => {
                        let (lo, hi) = if ka < kb { (a, b) } else { (b, a) };
                        let (va, vb) = (vert(lo), vert(hi));
                        let mid = (corner(lo[0], lo[1], lo[2]) + corner(hi[0], hi[1], hi[2])) * 0.5 - center;
                        let transit = center + mid.normalized().unwrap_or(dir) * radius;
                        let e = builder::circle_arc(&va, &vb, p3(transit));
                        edges.insert((ka.min(kb), ka.max(kb)), e.clone());
                        e
                    }
                };
                wire.push(if ka < kb { e } else { e.inverse() });
            }
            // The meridian runs pole to pole through the side away from the patch.
            let (np, sp, back) = (center + axis * radius, center - axis * radius, center - dir * radius);
            let meridian = builder::circle_arc(&builder::vertex(p3(np)), &builder::vertex(p3(sp)), p3(back));
            let mut surface =
                mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(meridian.oriented_curve(), p3(center), v3(axis))));
            // Outward normal at the patch centre.
            let probe = p3(center + dir * radius);
            use mt::{ParametricSurface3D, SearchParameter};
            if let Some((u, v)) = surface.search_parameter(probe, mt::SPHint2D::None, 100) {
                let n = surface.normal(u, v);
                if mt::InnerSpace::dot(n, v3(dir)) < 0.0 {
                    mt::Invertible::invert(&mut surface);
                }
            }
            out.push(mt::Face::new(vec![wire.into()], surface));
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Failed(format!("sphere: {e}")))?;
        Body::new(solid)
    })
}
