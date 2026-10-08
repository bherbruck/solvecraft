//! Helical sweeps (coils, springs) and lofts to a point (pyramids, cones).

use mt::builder;
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, Solid, p3};
use crate::build::{cap_and_close, uniform_wire};
use crate::{KernelError, Result, guard};

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rot(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Sweep a planar region's outer loop along a helix about `axis` through `axis_origin`:
/// `turns` revolutions (right-handed about `axis`), rising `pitch` per turn. The profile keeps
/// its attitude relative to the axis, as a screw motion does; between stations (3° apart) the
/// walls are ruled.
pub fn sweep_helix(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("helix axis".into()))?;
    if !(pitch.is_finite() && turns.is_finite() && turns > 0.0 && turns <= 200.0 && pitch.abs() < 1e7 && axis_origin.is_finite()) {
        return Err(KernelError::Invalid("helix: turns must be in (0, 200] and the pitch finite".into()));
    }
    if region.area().abs() < 1e-9 {
        return Err(KernelError::Invalid("profile has no area".into()));
    }
    // The profile must clear itself after one turn.
    let lp = region.outer.ccw();
    let along: Vec<f64> =
        lp.segs.iter().flat_map(|s| (0..8).map(move |i| s.point_at(i as f64 / 8.0))).map(|q| (plane.to_world(q) - axis_origin).dot(k)).collect();
    let extent = along.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - along.iter().cloned().fold(f64::INFINITY, f64::min);
    if turns > 1.0 && pitch.abs() <= extent {
        return Err(KernelError::Invalid(format!("the coil runs into itself: the pitch must exceed the section ({extent:.3} mm)")));
    }
    let n = ((turns * 120.0).ceil() as usize).clamp(4, 24_000);
    let total = std::f64::consts::TAU * turns;
    let mut wires = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = total * i as f64 / n as f64;
        let lift = k * (pitch * a / std::f64::consts::TAU);
        let o = axis_origin + rot(plane.origin - axis_origin, k, a) + lift;
        let pl = Plane { origin: o, x: rot(plane.x, k, a), y: rot(plane.y, k, a) };
        wires.push(uniform_wire(&pl, &lp)?);
    }
    guard("helix sweep", || {
        let mut faces: Vec<mt::Face> = Vec::new();
        for w in wires.windows(2) {
            let sides = builder::try_wire_homotopy(&w[0], &w[1]).map_err(|e| KernelError::Failed(format!("helix: {e}")))?;
            faces.extend(sides.face_iter().cloned());
        }
        let (Some(first), Some(last)) = (wires.first(), wires.last()) else { return Err(KernelError::Failed("helix".into())) };
        cap_and_close(faces, std::slice::from_ref(first), std::slice::from_ref(last))
    })
}

/// A solid from a planar loop of straight segments to an apex point off its plane (a pyramid).
pub fn loft_to_point(plane: &Plane, lp: &Loop2, apex: Vec3) -> Result<Body> {
    if !apex.is_finite() || lp.segs.is_empty() || lp.segs.len() > 10_000 {
        return Err(KernelError::Invalid("loft to a point: bad section or apex".into()));
    }
    if ((apex - plane.origin).dot(plane.normal())).abs() < 1e-9 {
        return Err(KernelError::Invalid("the apex lies in the section's plane".into()));
    }
    if lp.segs.iter().any(|s| !matches!(s, Seg2::Line { .. })) {
        return Err(KernelError::Invalid("not supported yet: lofting curved sections to a point".into()));
    }
    // Wind the base so its normal points away from the apex.
    let mut lp = lp.ccw();
    if (apex - plane.origin).dot(plane.normal()) > 0.0 {
        lp = lp.reversed();
    }
    guard("loft to point", || {
        let verts: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
        let top = builder::vertex(p3(apex));
        let m = verts.len();
        let base: Vec<mt::Edge> = (0..m).filter_map(|i| Some(builder::line(verts.get(i)?, verts.get((i + 1) % m)?))).collect();
        let sides: Vec<mt::Edge> = verts.iter().map(|v| builder::line(v, &top)).collect();
        let mut faces = Vec::with_capacity(m + 1);
        let bw: mt::Wire = base.iter().cloned().collect();
        faces.push(builder::try_attach_plane(&[bw]).map_err(|e| KernelError::Failed(format!("loft base: {e}")))?);
        for i in 0..m {
            let (Some(e), Some(up), Some(down)) = (base.get(i), sides.get((i + 1) % m), sides.get(i)) else { continue };
            // Triangle i: along the base edge, up to the apex, back down — opposite to the base's winding.
            let w: mt::Wire = vec![e.inverse(), down.clone(), up.inverse()].into();
            faces.push(builder::try_attach_plane(&[w]).map_err(|e| KernelError::Failed(format!("loft side: {e}")))?);
        }
        let shell: mt::Shell = faces.into();
        match Solid::try_new(vec![shell.clone()]) {
            Ok(s) => Body::new(s),
            Err(_) => {
                let inv: mt::Shell = shell.face_iter().map(|f| f.inverse()).collect();
                Body::new(Solid::try_new(vec![inv]).map_err(|e| KernelError::Failed(format!("loft to point: {e}")))?)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solvecraft_geom::Vec2;

    #[test]
    fn coil_volume_is_section_times_helix_length() {
        // A 2 mm circle in a plane through the Z axis, 20 mm out, 3 turns, pitch 8: a screw
        // motion of an axial section sweeps π r² · 2πR · turns (Fusion's spline helix: 4738.1).
        let pl = Plane::named("XZ").unwrap_or_else(|| Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z });
        let region = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 2.0), holes: vec![] };
        let b = sweep_helix(&pl, &region, Vec3::ZERO, Vec3::Z, 8.0, 3.0).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        let want = std::f64::consts::PI * 4.0 * std::f64::consts::TAU * 20.0 * 3.0;
        assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
        assert!(sweep_helix(&pl, &region, Vec3::ZERO, Vec3::Z, 3.0, 3.0).is_err(), "pitch below the section");
        assert!(sweep_helix(&pl, &region, Vec3::ZERO, Vec3::ZERO, 8.0, 3.0).is_err());
    }

    #[test]
    fn pyramid() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        let lp = Loop2::polygon(&[Vec2::new(-20.0, -20.0), Vec2::new(20.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)]);
        let b = loft_to_point(&pl, &lp, Vec3::new(0.0, 0.0, 30.0)).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        assert!((v - 1600.0 * 30.0 / 3.0).abs() < 1e-6, "{v}");
        assert_eq!(b.face_count(), 5);
        assert!(loft_to_point(&pl, &lp, Vec3::new(1.0, 1.0, 0.0)).is_err());
    }
}
