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
fn wire(plane: &Plane, lp: &Loop2) -> Result<mt::Wire> {
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
        let r = &Region2 { outer: snap_loop(&r.outer), holes: r.holes.iter().map(snap_loop).collect() };
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
    let radius = finite(radius, "radius")?;
    if radius <= 1e-6 {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    let plane = Plane { origin: center, ..Plane::XZ };
    let half = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
            Seg2::Line { a: Vec2::new(0.0, radius), b: Vec2::new(0.0, -radius) },
        ],
    };
    let mut v = revolve(&plane, &[Region2 { outer: half, holes: vec![] }], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?;
    v.pop().ok_or_else(|| KernelError::Failed("sphere".into()))
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
                Seg2::Line { .. } => Err(KernelError::Failed("not supported yet: tapered profiles mixing lines and arcs".into())),
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
    guard("revolve", || {
        let verts: Vec<mt::Vertex> = chain
            .iter()
            .map(|s| builder::vertex(p3(plane.to_world(s.start()))))
            .chain(chain.last().map(|s| builder::vertex(p3(plane.to_world(s.end())))))
            .collect();
        let mut edges: Vec<mt::Edge> = Vec::new();
        for (i, s) in chain.iter().enumerate() {
            let (Some(a), Some(b)) = (verts.get(i), verts.get(i + 1)) else { continue };
            edges.push(match *s {
                Seg2::Line { .. } => builder::line(a, b),
                Seg2::Arc { .. } => builder::circle_arc(a, b, p3(plane.to_world(s.mid()))),
            });
        }
        let w: mt::Wire = edges.into();
        let axis = plane.dir_to_world(d);
        let shell = builder::cone(&w, v3(axis), mt::Rad(std::f64::consts::TAU));
        let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Failed(format!("revolve: {e}")))?;
        let solid = if shift.len() > 0.0 { builder::translated(&solid, v3(shift)) } else { solid };
        Body::new(solid).map(Some)
    })
}
