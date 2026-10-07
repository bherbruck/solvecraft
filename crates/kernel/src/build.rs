//! Building bodies: extrude and revolve planar regions, primitives.

use monstertruck_modeling as mt;
use mt::builder;
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

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
                let mid = plane.to_world(s.mid());
                if sweep.abs() > std::f64::consts::PI * 1.5 {
                    let vm = builder::vertex(p3(mid));
                    let q1 = plane.to_world(s.point_at(0.25));
                    let q3 = plane.to_world(s.point_at(0.75));
                    edges.push(
                        builder::try_circle_arc(a, &vm, mt::builder::CircularArcConstraint::ThroughPoint(p3(q1)))
                            .map_err(|e| KernelError::Invalid(e.to_string()))?,
                    );
                    builder::try_circle_arc(&vm, b, mt::builder::CircularArcConstraint::ThroughPoint(p3(q3)))
                        .map_err(|e| KernelError::Invalid(e.to_string()))?
                } else {
                    builder::try_circle_arc(a, b, mt::builder::CircularArcConstraint::ThroughPoint(p3(mid)))
                        .map_err(|e| KernelError::Invalid(e.to_string()))?
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
    guard("face", || mt::profile::attach_plane_normalized(wires).map_err(|e| KernelError::Invalid(format!("profile: {e}"))))
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
        let solid: Solid = guard("extrude", || Ok(builder::extrude(&f, v3(n * (hi - lo)))))?;
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
        let f = face(plane, r)?;
        let full = angle.abs() >= std::f64::consts::TAU - 1e-9;
        let solid: Solid = guard("revolve", || {
            Ok(if full {
                builder::revolve(&f, p3(o), v3(axis), builder::SweepAngle::Closed, 4)
            } else {
                let div = ((angle.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(1, 8);
                builder::revolve(&f, p3(o), v3(axis), builder::SweepAngle::Partial(mt::Rad(angle)), div)
            })
        })?;
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
