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
    /// Per edge: a seam between two pieces of the same smooth surface (not drawn).
    #[serde(default)]
    pub seams: Vec<bool>,
    /// Per edge: the B-rep faces it bounds (one or two).
    #[serde(default)]
    pub edge_faces: Vec<Vec<u32>>,
}

/// Mass properties of a closed mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
}

impl Mesh {
    /// Edges bounding face `f`.
    pub fn face_edges(&self, f: u32) -> Vec<usize> {
        self.edge_faces.iter().enumerate().filter(|(_, fs)| fs.contains(&f)).map(|(i, _)| i).collect()
    }
    /// The edges reached from `e` through vertices where the next edge continues smoothly
    /// (angle between the end tangents below `max_angle`), including `e` itself.
    pub fn tangent_chain(&self, e: usize, max_angle: f64) -> Vec<usize> {
        // (end point, direction leaving the edge there). A polyline's end chord leans inward
        // on a curve by half the turn to the next chord; extrapolating the two end chords
        // recovers the curve's end tangent.
        let end_dir = |p0: Vec3, p1: Vec3, p2: Option<Vec3>| -> Option<Vec3> {
            let u1 = (p0 - p1).normalized()?;
            match p2.and_then(|p2| (p1 - p2).normalized()) {
                Some(u2) => (u1 * 1.5 - u2 * 0.5).normalized(),
                None => Some(u1),
            }
        };
        let ends = |i: usize| -> Option<[(Vec3, Vec3); 2]> {
            let p = self.edges.get(i)?;
            let n = p.len();
            let (a, a1, a2) = (*p.first()?, *p.get(1)?, p.get(2).copied().filter(|_| n > 2));
            let (b, b1) = (*p.last()?, *p.get(n.checked_sub(2)?)?);
            let b2 = n.checked_sub(3).and_then(|k| p.get(k)).copied();
            Some([(a, end_dir(a, a1, a2)?), (b, end_dir(b, b1, b2)?)])
        };
        let size = self.bounds().diagonal().max(1e-9);
        let tol = size * 1e-7;
        let cos = max_angle.cos();
        let mut chain = vec![e];
        let mut todo = vec![e];
        while let Some(cur) = todo.pop() {
            let Some(ce) = ends(cur) else { continue };
            for (i, _) in self.edges.iter().enumerate() {
                if chain.contains(&i) || self.seams.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let Some(ne) = ends(i) else { continue };
                // Smooth: leaving `cur` at a shared end continues into `i` (opposite directions).
                let smooth = ce.iter().any(|(p, d)| ne.iter().any(|(q, dn)| p.dist(*q) < tol && d.dot(-*dn) > cos));
                if smooth && chain.len() < 10_000 {
                    chain.push(i);
                    todo.push(i);
                }
            }
        }
        chain
    }
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

    /// Number of triangles a ray crosses (for inside/outside tests on closed meshes).
    pub fn ray_crossings(&self, o: Vec3, dir: Vec3) -> usize {
        let mut n = 0;
        for t in &self.triangles {
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
            if !(0.0..1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = dir.dot(q) * inv;
            if v < 0.0 || u + v >= 1.0 {
                continue;
            }
            if e2.dot(q) * inv > 0.0 {
                n += 1;
            }
        }
        n
    }

    /// Point inside a closed mesh (majority vote of three skewed rays).
    pub fn contains(&self, p: Vec3) -> bool {
        let dirs = [Vec3::new(0.5773, 0.5774, 0.5776), Vec3::new(-0.6123, 0.3141, 0.7254), Vec3::new(0.2718, -0.8414, 0.4673)];
        dirs.iter().filter(|d| self.ray_crossings(p, **d) % 2 == 1).count() >= 2
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
