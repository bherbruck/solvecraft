use serde::{Deserialize, Serialize};

use crate::Vec3;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb3 {
    fn default() -> Self {
        Aabb3::EMPTY
    }
}

impl Aabb3 {
    pub const EMPTY: Aabb3 = Aabb3 {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    pub fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Aabb3) -> Aabb3 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Aabb3 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn center(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { (self.min + self.max) * 0.5 }
    }
    pub fn size(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { self.max - self.min }
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
}

/// An indexed triangle mesh with per-vertex normals, a face id per triangle (the B-rep face it
/// came from) and edge polylines (the B-rep edges, for display and picking).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Triangles as vertex index triples (counter-clockwise seen from outside).
    pub triangles: Vec<[u32; 3]>,
    /// B-rep face index per triangle.
    pub tri_face: Vec<u32>,
    /// B-rep edges as polylines.
    pub edges: Vec<Vec<Vec3>>,
}

/// Mass properties of a closed mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
}

impl Mesh {
    pub fn tri(&self, t: &[u32; 3]) -> Option<[Vec3; 3]> {
        Some([*self.positions.get(t[0] as usize)?, *self.positions.get(t[1] as usize)?, *self.positions.get(t[2] as usize)?])
    }

    pub fn bounds(&self) -> Aabb3 {
        let mut b = Aabb3::EMPTY;
        for p in &self.positions {
            b.add(*p);
        }
        b
    }

    /// Signed volume (divergence theorem), surface area and volume centroid.
    pub fn measure(&self) -> MeshMeasure {
        let (mut v6, mut area, mut c) = (0.0, 0.0, Vec3::ZERO);
        for t in &self.triangles {
            let Some([a, b, cc]) = self.tri(t) else { continue };
            let d = a.dot(b.cross(cc));
            v6 += d;
            c += (a + b + cc) * d;
            area += (b - a).cross(cc - a).len() * 0.5;
        }
        let volume = v6 / 6.0;
        let centroid = if v6.abs() > 1e-12 { c / (4.0 * v6) } else { self.bounds().center() };
        MeshMeasure { volume, area, centroid }
    }

    /// Append another mesh (face ids offset by `face_offset`).
    pub fn append(&mut self, o: &Mesh, face_offset: u32) {
        let base = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.extend_from_slice(&o.positions);
        self.normals.extend_from_slice(&o.normals);
        self.triangles.extend(o.triangles.iter().map(|t| [t[0].saturating_add(base), t[1].saturating_add(base), t[2].saturating_add(base)]));
        self.tri_face.extend(o.tri_face.iter().map(|f| f.saturating_add(face_offset)));
        self.edges.extend(o.edges.iter().cloned());
    }

    /// Closest intersection of a ray with the mesh: (distance along `dir`, triangle index).
    pub fn raycast(&self, o: Vec3, dir: Vec3) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.triangles.iter().enumerate() {
            let Some([a, b, c]) = self.tri(t) else { continue };
            let (e1, e2) = (b - a, c - a);
            let p = dir.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-14 {
                continue;
            }
            let inv = 1.0 / det;
            let s = o - a;
            let u = s.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = dir.dot(q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let t = e2.dot(q) * inv;
            if t > 1e-9 && best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, i));
            }
        }
        best
    }
}
