use serde::{Deserialize, Serialize};
use solvecraft_geom::{Aabb3, Vec3};

/// Column-major 4×4 matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (c, col) in r.iter_mut().enumerate() {
            for (rr, cell) in col.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.0[k][rr] * o.0[c][k]).sum();
            }
        }
        Mat4(r)
    }
    /// Transform a point; returns (x, y, z, w).
    pub fn apply(&self, p: Vec3) -> [f64; 4] {
        let v = [p.x, p.y, p.z, 1.0];
        let mut out = [0.0; 4];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|c| self.0[c][r] * v[c]).sum();
        }
        out
    }
    pub fn to_f32(&self) -> [[f32; 4]; 4] {
        self.0.map(|c| c.map(|x| x as f32))
    }
}

/// Named view orientations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardView {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Iso,
}

impl StandardView {
    pub fn parse(s: &str) -> Option<StandardView> {
        Some(match s.to_ascii_lowercase().as_str() {
            "front" => StandardView::Front,
            "back" => StandardView::Back,
            "top" => StandardView::Top,
            "bottom" => StandardView::Bottom,
            "left" => StandardView::Left,
            "right" => StandardView::Right,
            "iso" | "home" | "isometric" => StandardView::Iso,
            _ => return None,
        })
    }
    /// (yaw, pitch) in radians. Yaw 0 looks along +Y (front view); the iso view looks from the
    /// front-right-top corner. Pitch is the elevation above the XY plane.
    pub fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            StandardView::Front => (0.0, 0.0),
            StandardView::Back => (PI, 0.0),
            StandardView::Right => (-FRAC_PI_2, 0.0),
            StandardView::Left => (FRAC_PI_2, 0.0),
            StandardView::Top => (0.0, FRAC_PI_2),
            StandardView::Bottom => (0.0, -FRAC_PI_2),
            StandardView::Iso => (-std::f64::consts::FRAC_PI_4, 0.6154797086703874),
        }
    }
}

/// Smooth ease-in-out on [0, 1] (cubic).
pub fn ease_in_out(t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// A unit quaternion (w, x, y, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quat {
    /// From an orthonormal right-handed basis (the columns of a rotation matrix).
    pub fn from_basis(c0: Vec3, c1: Vec3, c2: Vec3) -> Quat {
        let (m00, m11, m22) = (c0.x, c1.y, c2.z);
        let tr = m00 + m11 + m22;
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (c1.z - c2.y) / s, y: (c2.x - c0.z) / s, z: (c0.y - c1.x) / s }
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quat { w: (c1.z - c2.y) / s, x: 0.25 * s, y: (c1.x + c0.y) / s, z: (c2.x + c0.z) / s }
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quat { w: (c2.x - c0.z) / s, x: (c1.x + c0.y) / s, y: 0.25 * s, z: (c2.y + c1.z) / s }
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quat { w: (c0.y - c1.x) / s, x: (c2.x + c0.z) / s, y: (c2.y + c1.z) / s, z: 0.25 * s }
        };
        q.normalized()
    }
    pub fn dot(self, o: Quat) -> f64 {
        self.w * o.w + self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn scale(self, k: f64) -> Quat {
        Quat { w: self.w * k, x: self.x * k, y: self.y * k, z: self.z * k }
    }
    fn add(self, o: Quat) -> Quat {
        Quat { w: self.w + o.w, x: self.x + o.x, y: self.y + o.y, z: self.z + o.z }
    }
    pub fn normalized(self) -> Quat {
        let l = self.len();
        if l > 1e-300 && l.is_finite() { self.scale(1.0 / l) } else { Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 } }
    }
    /// Shortest-arc spherical interpolation.
    pub fn slerp(self, o: Quat, t: f64) -> Quat {
        let mut d = self.dot(o);
        let o = if d < 0.0 {
            d = -d;
            o.scale(-1.0)
        } else {
            o
        };
        if d > 0.9995 {
            return self.scale(1.0 - t).add(o.scale(t)).normalized();
        }
        let th = d.clamp(-1.0, 1.0).acos();
        let s = th.sin();
        self.scale(((1.0 - t) * th).sin() / s).add(o.scale((t * th).sin() / s)).normalized()
    }
    /// The rotated basis vectors (columns of the rotation matrix).
    pub fn basis(self) -> (Vec3, Vec3, Vec3) {
        let Quat { w, x, y, z } = self;
        (
            Vec3::new(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
            Vec3::new(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
            Vec3::new(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)),
        )
    }
}

/// A camera move in progress: from one camera to another over `duration` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraAnim {
    pub from: Camera,
    pub to: Camera,
    pub start: f64,
    pub duration: f64,
}

impl CameraAnim {
    /// The usual length of a view change, in seconds.
    pub const DURATION: f64 = 0.5;
    pub fn new(from: Camera, to: Camera, now: f64) -> CameraAnim {
        CameraAnim { from, to, start: now, duration: CameraAnim::DURATION }
    }
    /// The camera at time `now`, and whether the move has finished.
    pub fn sample(&self, now: f64) -> (Camera, bool) {
        let t = if self.duration > 0.0 { (now - self.start) / self.duration } else { 1.0 };
        if !(t < 1.0) {
            return (self.to, true);
        }
        (Camera::interpolate(&self.from, &self.to, ease_in_out(t)), false)
    }
}

/// Orbit camera around `target` (Z up).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation about Z; 0 = looking along +Y (from the front).
    pub yaw: f64,
    /// Elevation above the XY plane, in (−π/2, π/2].
    pub pitch: f64,
    /// Distance from target to eye.
    pub distance: f64,
    /// Vertical field of view (radians); 0 = orthographic.
    pub fov: f64,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = StandardView::Iso.angles();
        Camera { target: Vec3::ZERO, yaw, pitch, distance: 200.0, fov: 0.0 }
    }
}

impl Camera {
    /// Direction from the target toward the eye.
    pub fn back(&self) -> Vec3 {
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        Vec3::new(-self.yaw.sin() * cp, -self.yaw.cos() * cp, sp).normalized().unwrap_or(Vec3::Z)
    }
    pub fn eye(&self) -> Vec3 {
        self.target + self.back() * self.distance
    }
    /// Camera basis: right, up, back (orthonormal).
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let b = self.back();
        let right = Vec3::new(self.yaw.cos(), -self.yaw.sin(), 0.0);
        let up = b.cross(right).normalized().unwrap_or(Vec3::Z);
        (right, up, b)
    }
    pub fn view(&self) -> Mat4 {
        let (r, u, b) = self.basis();
        let e = self.eye();
        Mat4([[r.x, u.x, b.x, 0.0], [r.y, u.y, b.y, 0.0], [r.z, u.z, b.z, 0.0], [-r.dot(e), -u.dot(e), -b.dot(e), 1.0]])
    }
    /// Half the visible height at the target distance.
    pub fn half_height(&self) -> f64 {
        if self.fov > 0.0 { self.distance * (self.fov * 0.5).tan() } else { self.distance * 0.4 }
    }
    /// Near/far planes that keep a scene of radius `radius` around the target visible.
    pub fn clip(&self, radius: f64) -> (f64, f64) {
        let r = radius.max(1e-3);
        if self.fov > 0.0 {
            ((self.distance - r * 2.0).max(self.distance * 1e-3).max(1e-3), self.distance + r * 2.0)
        } else {
            (self.distance - r * 4.0 - 1.0, self.distance + r * 4.0 + 1.0)
        }
    }
    /// Projection to clip space (OpenGL/wgpu style, depth 0..1).
    pub fn proj(&self, aspect: f64, near: f64, far: f64) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 1e-6 { aspect } else { 1.0 };
        let (n, f) = (near, far.max(near + 1e-6));
        if self.fov > 0.0 {
            let t = 1.0 / (self.fov * 0.5).tan();
            Mat4([[t / aspect, 0.0, 0.0, 0.0], [0.0, t, 0.0, 0.0], [0.0, 0.0, f / (n - f), -1.0], [0.0, 0.0, n * f / (n - f), 0.0]])
        } else {
            let h = self.half_height();
            let w = h * aspect;
            Mat4([[1.0 / w, 0.0, 0.0, 0.0], [0.0, 1.0 / h, 0.0, 0.0], [0.0, 0.0, 1.0 / (n - f), 0.0], [0.0, 0.0, n / (n - f), 1.0]])
        }
    }
    pub fn view_proj(&self, aspect: f64, radius: f64) -> Mat4 {
        let (n, f) = self.clip(radius);
        self.proj(aspect, n, f).mul(&self.view())
    }
    /// World point → pixel coordinates (x right, y down) and depth (0 near … 1 far).
    pub fn to_screen(&self, p: Vec3, w: f64, h: f64, radius: f64) -> Option<(f64, f64, f64)> {
        let c = self.view_proj(w / h.max(1.0), radius).apply(p);
        if c[3].abs() < 1e-12 {
            return None;
        }
        let (x, y, z) = (c[0] / c[3], c[1] / c[3], c[2] / c[3]);
        (c[3] > 0.0).then_some(((x + 1.0) * 0.5 * w, (1.0 - y) * 0.5 * h, z))
    }
    /// Picking ray through pixel (x, y): origin and unit direction.
    pub fn ray(&self, x: f64, y: f64, w: f64, h: f64) -> (Vec3, Vec3) {
        let (r, u, b) = self.basis();
        let hh = self.half_height();
        let aspect = w / h.max(1.0);
        let nx = (x / w.max(1.0)) * 2.0 - 1.0;
        let ny = 1.0 - (y / h.max(1.0)) * 2.0;
        if self.fov > 0.0 {
            let d = (r * (nx * hh * aspect) + u * (ny * hh) - b * self.distance).normalized().unwrap_or(-b);
            (self.eye(), d)
        } else {
            let o = self.target + r * (nx * hh * aspect) + u * (ny * hh) + b * (self.distance * 4.0);
            (o, -b)
        }
    }
    /// Zoom by `factor` (< 1 zooms in) toward a world point that stays put on screen.
    pub fn zoom_at(&mut self, factor: f64, anchor: Option<Vec3>) {
        let f = if factor.is_finite() { factor.clamp(0.05, 20.0) } else { 1.0 };
        let nd = (self.distance * f).clamp(1e-3, 1e7);
        if let Some(a) = anchor {
            self.target = a + (self.target - a) * (nd / self.distance);
        }
        self.distance = nd;
    }
    /// Pan by a screen delta in pixels for a viewport of height `h`.
    pub fn pan(&mut self, dx: f64, dy: f64, h: f64) {
        let (r, u, _) = self.basis();
        let k = 2.0 * self.half_height() / h.max(1.0);
        self.target = self.target - r * (dx * k) + u * (dy * k);
    }
    /// Orbit by pixel deltas. The model follows the cursor: dragging right turns the side
    /// facing the viewer to the right.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let lim = std::f64::consts::FRAC_PI_2 - 1e-4;
        self.yaw = (self.yaw + dx * 0.008) % std::f64::consts::TAU;
        self.pitch = (self.pitch + dy * 0.008).clamp(-lim, lim);
    }
    pub fn set_view(&mut self, v: StandardView) {
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let (y, p) = v.angles();
        self.yaw = y;
        self.pitch = p.clamp(-lim, lim);
    }
    /// Frame a bounding box.
    pub fn fit(&mut self, b: &Aabb3) {
        if b.is_empty() {
            return;
        }
        self.target = b.center();
        let r = (b.diagonal() * 0.5).max(1.0);
        self.distance = if self.fov > 0.0 { r / (self.fov * 0.5).sin() * 1.1 } else { r / 0.4 * 1.15 };
    }
    /// The same camera looking from direction `back` (target → eye). Straight up or down
    /// keeps the yaw, rounded to the nearest quarter turn so the view stays square.
    pub fn looking_from(&self, back: Vec3) -> Camera {
        let Some(b) = back.normalized() else { return *self };
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let pitch = b.z.clamp(-1.0, 1.0).asin().clamp(-lim, lim);
        let yaw = if b.x.hypot(b.y) < 1e-6 {
            let q = std::f64::consts::FRAC_PI_2;
            (self.yaw / q).round() * q
        } else {
            (-b.x).atan2(-b.y)
        };
        Camera { yaw, pitch, ..*self }
    }
    /// Orientation as a unit quaternion (camera → world rotation).
    pub fn orientation(&self) -> Quat {
        let (r, u, b) = self.basis();
        Quat::from_basis(r, u, b)
    }
    /// Blend between two cameras at `s` in [0, 1]: orientation by quaternion slerp, target,
    /// distance and field of view linearly. The ends are returned exactly.
    pub fn interpolate(a: &Camera, b: &Camera, s: f64) -> Camera {
        if !(s > 0.0) {
            return *a;
        }
        if s >= 1.0 {
            return *b;
        }
        let q = a.orientation().slerp(b.orientation(), s);
        let (r, _, back) = q.basis();
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let pitch = back.z.clamp(-1.0, 1.0).asin().clamp(-lim, lim);
        // The camera has no roll: follow the slerped view direction, and near straight up/down
        // (where the direction has no heading) the slerped right vector.
        let yaw = if back.x.hypot(back.y) > 1e-3 {
            (-back.x).atan2(-back.y)
        } else if r.x.hypot(r.y) > 1e-9 {
            (-r.y).atan2(r.x)
        } else {
            a.yaw + (b.yaw - a.yaw) * s
        };
        Camera {
            target: a.target + (b.target - a.target) * s,
            yaw,
            pitch,
            distance: a.distance + (b.distance - a.distance) * s,
            fov: a.fov + (b.fov - a.fov) * s,
        }
    }
    pub fn is_valid(&self) -> bool {
        self.target.is_finite() && self.yaw.is_finite() && self.pitch.is_finite() && self.distance.is_finite() && self.distance > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_view_projects_target_to_centre() {
        let mut c = Camera { target: Vec3::new(1.0, 2.0, 3.0), distance: 50.0, ..Default::default() };
        c.set_view(StandardView::Front);
        let (x, y, z) = c.to_screen(c.target, 800.0, 600.0, 10.0).unwrap();
        assert!((x - 400.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6 && (0.0..=1.0).contains(&z));
        // Front view looks along +Y: +X is to the right, +Z up.
        let (xr, _, _) = c.to_screen(c.target + Vec3::X, 800.0, 600.0, 10.0).unwrap();
        let (_, yu, _) = c.to_screen(c.target + Vec3::Z, 800.0, 600.0, 10.0).unwrap();
        assert!(xr > 400.0 && yu < 300.0);
        let (o, d) = c.ray(400.0, 300.0, 800.0, 600.0);
        assert!(d.dist(Vec3::Y) < 1e-9, "{d:?}");
        assert!((o - c.target).cross(d).len() < 1e-6);
    }

    #[test]
    fn horizontal_orbit_follows_the_cursor() {
        let mut c = Camera { distance: 50.0, ..Default::default() };
        c.set_view(StandardView::Front);
        // Front view looks along +Y, so -Y is the side facing the viewer.
        let near = Vec3::new(0.0, -1.0, 0.0);
        let x0 = c.to_screen(near, 800.0, 600.0, 10.0).unwrap().0;
        c.orbit(20.0, 0.0);
        let x1 = c.to_screen(near, 800.0, 600.0, 10.0).unwrap().0;
        assert!(x1 > x0, "dragging right must move the near side right: {x0} -> {x1}");
    }

    #[test]
    fn view_animation_interpolates() {
        let mut a = Camera { target: Vec3::new(1.0, 2.0, 3.0), distance: 80.0, ..Default::default() };
        a.set_view(StandardView::Iso);
        let mut b = Camera { target: Vec3::new(-5.0, 0.0, 10.0), distance: 120.0, ..Default::default() };
        b.set_view(StandardView::Top);
        let anim = CameraAnim::new(a, b, 10.0);
        // Endpoints are exact.
        assert_eq!(anim.sample(10.0).0, a);
        assert_eq!(anim.sample(10.0 + CameraAnim::DURATION), (b, true));
        assert_eq!(anim.sample(99.0), (b, true));
        // Midpoint: a valid orientation half way between (equal angles to both ends).
        let (m, done) = anim.sample(10.0 + CameraAnim::DURATION / 2.0);
        assert!(!done && m.is_valid());
        let q = m.orientation();
        assert!((q.len() - 1.0).abs() < 1e-12);
        let (qa, qb) = (a.orientation(), b.orientation());
        let qm = qa.slerp(qb, 0.5);
        assert!((qm.dot(qa).abs() - qm.dot(qb).abs()).abs() < 1e-12);
        // The camera keeps no roll but follows the slerped view direction.
        let (_, _, back) = qm.basis();
        assert!(m.back().dist(back) < 1e-9, "{:?} vs {back:?}", m.back());
        assert!(m.target.dist(Vec3::new(-2.0, 1.0, 6.5)) < 1e-9 && (m.distance - 100.0).abs() < 1e-9);
        // Quaternion round trip of a basis.
        let (r, u, bk) = a.basis();
        let (r2, u2, b2) = a.orientation().basis();
        assert!(r.dist(r2) < 1e-12 && u.dist(u2) < 1e-12 && bk.dist(b2) < 1e-12);
        // Ease: fixed ends, symmetric, monotonic.
        assert_eq!((ease_in_out(0.0), ease_in_out(1.0), ease_in_out(0.5)), (0.0, 1.0, 0.5));
        assert!(ease_in_out(0.25) < 0.25 && ease_in_out(0.75) > 0.75);
        // Looking from a corner direction.
        let c = a.looking_from(Vec3::new(1.0, -1.0, 1.0));
        assert!(c.back().dist(Vec3::new(1.0, -1.0, 1.0).normalized().unwrap()) < 1e-9);
    }

    #[test]
    fn top_view_and_perspective() {
        let mut c = Camera { distance: 100.0, fov: 0.8, ..Default::default() };
        c.set_view(StandardView::Top);
        let (xr, _, _) = c.to_screen(Vec3::X, 800.0, 600.0, 10.0).unwrap();
        let (_, yu, _) = c.to_screen(Vec3::Y, 800.0, 600.0, 10.0).unwrap();
        assert!(xr > 400.0 && yu < 300.0, "{xr} {yu}");
        let near = c.to_screen(Vec3::new(0.0, 0.0, 10.0), 800.0, 600.0, 20.0).unwrap().2;
        let far = c.to_screen(Vec3::new(0.0, 0.0, -10.0), 800.0, 600.0, 20.0).unwrap().2;
        assert!(near < far);
        let (o, d) = c.ray(600.0, 300.0, 800.0, 600.0);
        let hit = o + d * (o.z / -d.z);
        let (sx, sy, _) = c.to_screen(hit, 800.0, 600.0, 20.0).unwrap();
        assert!((sx - 600.0).abs() < 1e-6 && (sy - 300.0).abs() < 1e-6);
    }

    #[test]
    fn fit_and_hostile() {
        let mut c = Camera::default();
        let b = Aabb3 { min: Vec3::ZERO, max: Vec3::new(40.0, 30.0, 20.0) };
        c.fit(&b);
        assert!(c.target.dist(Vec3::new(20.0, 15.0, 10.0)) < 1e-9);
        c.zoom_at(f64::NAN, None);
        c.orbit(1e9, -1e9);
        assert!(c.is_valid());
        c.fit(&Aabb3::EMPTY);
        assert!(c.is_valid());
    }
}
