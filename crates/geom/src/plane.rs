use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3};

/// A sketch plane: an origin and two orthonormal in-plane axes. The normal is `x × y`.
///
/// World point of sketch point `(u, v)` = `origin + u·x + v·y`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: Vec3,
    #[serde(rename = "x_dir")]
    pub x: Vec3,
    #[serde(rename = "y_dir")]
    pub y: Vec3,
}

impl Plane {
    /// Top: sketch x = +X, y = +Y, normal +Z.
    pub const XY: Plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
    /// Front: sketch x = +X, y = −Z, normal +Y.
    pub const XZ: Plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::new(0.0, 0.0, -1.0) };
    /// Right: sketch x = +Y, y = +Z, normal +X.
    pub const YZ: Plane = Plane { origin: Vec3::ZERO, x: Vec3::Y, y: Vec3::Z };

    /// Plane through `origin` with the given axes, re-orthonormalised. `None` if degenerate.
    pub fn new(origin: Vec3, x: Vec3, y: Vec3) -> Option<Plane> {
        let x = x.normalized()?;
        let n = x.cross(y).normalized()?;
        let y = n.cross(x).normalized()?;
        origin.is_finite().then_some(Plane { origin, x, y })
    }

    /// Plane through `origin` with normal `n` (in-plane axes chosen deterministically).
    pub fn from_normal(origin: Vec3, n: Vec3) -> Option<Plane> {
        let n = n.normalized()?;
        // Horizontal planes keep world X as sketch x; others use the horizontal direction Z × n.
        let cand = if n.dot(Vec3::Z).abs() > 0.9 { Vec3::X } else { Vec3::Z.cross(n) };
        let x = (cand - n * cand.dot(n)).normalized()?;
        Plane::new(origin, x, n.cross(x))
    }

    /// Named origin plane: `XY`, `XZ`, `YZ` (case-insensitive).
    pub fn named(name: &str) -> Option<Plane> {
        match name.to_ascii_uppercase().as_str() {
            "XY" | "TOP" => Some(Plane::XY),
            "XZ" | "FRONT" => Some(Plane::XZ),
            "YZ" | "RIGHT" => Some(Plane::YZ),
            _ => None,
        }
    }

    pub fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }
    pub fn to_world(&self, p: Vec2) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y
    }
    pub fn dir_to_world(&self, d: Vec2) -> Vec3 {
        self.x * d.x + self.y * d.y
    }
    pub fn to_local(&self, p: Vec3) -> Vec2 {
        let d = p - self.origin;
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }
    /// Signed distance of `p` above the plane (along the normal).
    pub fn height(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.normal())
    }
    /// The plane moved along its normal by `d`.
    pub fn offset(&self, d: f64) -> Plane {
        Plane { origin: self.origin + self.normal() * d, ..*self }
    }
    /// Intersection of the ray `o + t·dir` with the plane (t may be negative).
    pub fn intersect_ray(&self, o: Vec3, dir: Vec3) -> Option<Vec3> {
        let n = self.normal();
        let den = dir.dot(n);
        if den.abs() < 1e-12 {
            return None;
        }
        let t = (self.origin - o).dot(n) / den;
        t.is_finite().then(|| o + dir * t)
    }
}
