use serde::Serialize;
use solvecraft_geom::{Mesh, Vec3};
use truck_meshalgo::prelude::*;
use truck_modeling as mt;

use crate::{KernelError, Result, guard};

pub(crate) type Solid = mt::Solid;

/// A solid body (one or more closed shells), or a mesh body (triangles only, like an imported
/// 3MF or STL: it renders, measures and exports, but B-rep features cannot use it). Cheap to
/// clone; never mutated in place.
#[derive(Clone, Debug)]
pub struct Body {
    pub(crate) solid: std::sync::Arc<Solid>,
    /// Set for mesh bodies (`solid` is then empty).
    pub(crate) mesh: Option<std::sync::Arc<crate::meshbody::TriMesh>>,
    /// Display colour (linear RGB 0..1) from an imported file; operations drop it.
    pub(crate) color: Option<[f32; 3]>,
}

/// A B-rep edge as seen by the rest of the application.
#[derive(Clone, Debug, Serialize)]
pub struct EdgeInfo {
    pub index: usize,
    /// Polyline approximation.
    pub points: Vec<Vec3>,
    /// Point halfway along the polyline (used to re-find the edge).
    pub mid: Vec3,
    pub length: f64,
}

/// A B-rep face.
#[derive(Clone, Debug, Serialize)]
pub struct FaceInfo {
    pub index: usize,
    pub area: f64,
    pub centroid: Vec3,
    /// Unit normal if the face is planar.
    pub plane_normal: Option<Vec3>,
}

type MeshedShell = truck_topology::Shell<mt::Point3, PolylineCurve<mt::Point3>, Option<PolygonMesh>>;

/// Triangulate each shell (not the solid: truck's solid constructor panics on the open or
/// non-manifold shells an import can produce). Boundaries that lie on their surface only within
/// tolerance (imports, boolean results) are projected onto it.
fn mesh_shells(solid: &Solid, tol: f64) -> Vec<MeshedShell> {
    solid
        .boundaries()
        .iter()
        // The robust search tries the exact one first, so exact geometry costs nothing extra; a
        // face can also come back as an empty mesh rather than none (boolean results with
        // approximate intersection curves), so always use it.
        .map(|sh| sh.robust_triangulation(tol))
        .collect()
}

pub(crate) fn p3(v: Vec3) -> mt::Point3 {
    mt::Point3::new(v.x, v.y, v.z)
}
pub(crate) fn v3(v: Vec3) -> mt::Vector3 {
    mt::Vector3::new(v.x, v.y, v.z)
}
pub(crate) fn from_p3(p: mt::Point3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn polyline_mid(pts: &[Vec3]) -> (Vec3, f64) {
    let len: f64 = pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
            let l = a.dist(*b);
            if acc + l >= len * 0.5 && l > 0.0 {
                return (a.lerp(*b, (len * 0.5 - acc) / l), len);
            }
            acc += l;
        }
    }
    (pts.first().copied().unwrap_or_default(), len)
}

impl Body {
    pub(crate) fn new(mut solid: Solid) -> Result<Body> {
        if solid.boundaries().is_empty() {
            return Err(KernelError::Failed("empty result".into()));
        }
        // Orient outward: a closed solid must have positive volume.
        let v = guard("orient", || Ok(mesh_shells(&solid, 1.0).iter().map(|s| s.to_polygon().volume()).sum::<f64>()))?;
        if v < 0.0 {
            solid.not();
        }
        Ok(Body { solid: std::sync::Arc::new(solid), mesh: None, color: None })
    }

    /// A deep copy of the solid that can be mutated without affecting other bodies.
    pub(crate) fn deep_copy(&self) -> Solid {
        mt::builder::clone(&*self.solid)
    }

    /// Is this a mesh body (no B-rep)?
    pub fn is_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    /// Error for operations that need a B-rep.
    pub(crate) fn require_brep(&self, what: &str) -> Result<()> {
        if self.is_mesh() {
            Err(KernelError::Invalid(format!("{what} needs a solid body; mesh bodies can only be moved, measured and exported")))
        } else {
            Ok(())
        }
    }

    /// Display colour from an imported file.
    pub fn color(&self) -> Option<[f32; 3]> {
        self.color
    }

    /// The same body with a display colour.
    pub fn with_color(mut self, c: Option<[f32; 3]>) -> Body {
        self.color = c.filter(|c| c.iter().all(|x| x.is_finite())).map(|c| c.map(|x| x.clamp(0.0, 1.0)));
        self
    }

    /// Characteristic size (bounding box diagonal) for tolerances.
    pub fn size(&self) -> f64 {
        let mut b = solvecraft_geom::Aabb3::EMPTY;
        if let Some(m) = &self.mesh {
            for p in &m.positions {
                b.add(*p);
            }
        }
        for v in self.solid.vertex_iter() {
            b.add(from_p3(v.point()));
        }
        let d = b.diagonal();
        if d.is_finite() && d > 0.0 { d } else { 1.0 }
    }

    /// Distinct edges in a stable order (the order of `edges()` and `tessellate().edges`).
    pub(crate) fn unique_edges(solid: &Solid) -> Vec<mt::Edge> {
        let mut seen = std::collections::HashSet::new();
        solid.edge_iter().filter(|e| seen.insert(e.id())).collect()
    }

    /// Triangulate with chord tolerance `tol` (mm). Triangles carry their face index.
    pub fn tessellate(&self, tol: f64) -> Result<Mesh> {
        let tol = if tol.is_finite() && tol > 0.0 { tol } else { 0.05 };
        let tol = tol.max(self.size() * 1e-6);
        if let Some(m) = &self.mesh {
            return Ok(m.to_mesh());
        }
        guard("tessellate", || {
            let meshed = mesh_shells(&self.solid, tol);
            let mut out = Mesh::default();
            for (fi, face) in meshed.iter().flat_map(|s| s.face_iter()).enumerate() {
                let Some(pm) = face.surface() else { continue };
                let flip = !face.orientation();
                let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
                let pos = pm.positions();
                let nor = pm.normals();
                // Expand to per-corner vertices so normals stay per face (crisp edges).
                for tri in pm.faces().triangle_iter() {
                    let mut idx = [0u32; 3];
                    for (k, v) in tri.iter().enumerate() {
                        let Some(p) = pos.get(v.pos) else { continue };
                        let n = v.nor.and_then(|ni| nor.get(ni)).map(|n| Vec3::new(n.x, n.y, n.z)).unwrap_or(Vec3::Z);
                        let n = if flip { -n } else { n };
                        out.positions.push(Vec3::new(p.x, p.y, p.z));
                        out.normals.push(n);
                        if let Some(slot) = idx.get_mut(k) {
                            *slot = u32::try_from(out.positions.len() - 1).unwrap_or(0);
                        }
                    }
                    let t = if flip { [idx[0], idx[2], idx[1]] } else { idx };
                    out.triangles.push(t);
                    out.tri_face.push(u32::try_from(fi).unwrap_or(u32::MAX));
                }
                let _ = base;
            }
            // Fill missing normals from geometry.
            for t in &out.triangles {
                let (Some(a), Some(b), Some(c)) =
                    (out.positions.get(t[0] as usize), out.positions.get(t[1] as usize), out.positions.get(t[2] as usize))
                else {
                    continue;
                };
                let fnrm = (b - a).cross(c - a).normalized().unwrap_or(Vec3::Z);
                for k in t {
                    if let Some(n) = out.normals.get_mut(*k as usize)
                        && (n.dot(fnrm) < -0.2 || n.len() < 0.5)
                    {
                        *n = fnrm;
                    }
                }
            }
            let mut seen = std::collections::HashMap::new();
            for e in meshed.iter().flat_map(|s| s.edge_iter()) {
                if seen.contains_key(&e.id()) {
                    continue;
                }
                seen.insert(e.id(), out.edges.len());
                let c = e.oriented_curve();
                out.edges.push(c.0.iter().map(|p| Vec3::new(p.x, p.y, p.z)).collect());
            }
            // Which faces each edge bounds.
            out.edge_faces = vec![Vec::new(); out.edges.len()];
            for (fi, face) in meshed.iter().flat_map(|s| s.face_iter()).enumerate() {
                for e in face.edge_iter() {
                    if let Some(slot) = seen.get(&e.id()).and_then(|i| out.edge_faces.get_mut(*i)) {
                        let f = u32::try_from(fi).unwrap_or(u32::MAX);
                        if !slot.contains(&f) {
                            slot.push(f);
                        }
                    }
                }
            }
            Ok(out)
        })
    }

    /// Display mesh: like [`Body::tessellate`], with seam edges flagged.
    pub fn display_mesh(&self, tol: f64) -> Result<Mesh> {
        let mut m = self.tessellate(tol)?;
        if self.is_mesh() {
            return Ok(m);
        }
        m.seams = crate::topo::seam_flags(self, &m);
        Ok(m)
    }

    /// Edges with polylines (index = position in [`Body::tessellate`]'s `edges`).
    pub fn edges(&self, tol: f64) -> Result<Vec<EdgeInfo>> {
        let m = self.tessellate(tol)?;
        Ok(m.edges
            .into_iter()
            .enumerate()
            .map(|(index, points)| {
                let (mid, length) = polyline_mid(&points);
                EdgeInfo { index, points, mid, length }
            })
            .collect())
    }

    /// Faces with area and centroid.
    pub fn faces(&self, tol: f64) -> Result<Vec<FaceInfo>> {
        let m = self.tessellate(tol)?;
        let nf = self.face_count();
        let mut acc: Vec<(f64, Vec3, Vec3, bool)> = vec![(0.0, Vec3::ZERO, Vec3::ZERO, true); nf];
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, b, c]) = m.tri(t) else { continue };
            let cr = (b - a).cross(c - a);
            let ar = cr.len() * 0.5;
            if let Some(slot) = acc.get_mut(*f as usize) {
                slot.0 += ar;
                slot.1 += (a + b + c) * (ar / 3.0);
                if let Some(n) = cr.normalized() {
                    if slot.2 == Vec3::ZERO {
                        slot.2 = n;
                    } else if slot.2.dot(n) < 1.0 - 1e-6 {
                        slot.3 = false;
                    }
                }
            }
        }
        Ok(acc
            .into_iter()
            .enumerate()
            .map(|(index, (area, c, n, planar))| FaceInfo {
                index,
                area,
                centroid: if area > 0.0 { c / area } else { Vec3::ZERO },
                plane_normal: (planar && n != Vec3::ZERO).then_some(n),
            })
            .collect())
    }

    /// Index of the edge closest to `p` (by polyline distance).
    pub fn nearest_edge(&self, p: Vec3, tol: f64) -> Result<Option<(usize, f64)>> {
        let edges = self.edges(tol)?;
        let mut best: Option<(usize, f64)> = None;
        for e in &edges {
            for w in e.points.windows(2) {
                if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                    let d = p.dist_to_segment(*a, *b);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((e.index, d));
                    }
                }
            }
        }
        Ok(best)
    }

    /// Is the point inside the solid?
    pub fn contains(&self, p: Vec3) -> bool {
        self.tessellate(self.size() * 2e-3).map(|m| m.contains(p)).unwrap_or(false)
    }

    /// B-rep faces (a mesh body is one face; [`crate::measure`] counts its triangles).
    pub fn face_count(&self) -> usize {
        match &self.mesh {
            Some(_) => 1,
            None => self.solid.face_iter().count(),
        }
    }
    /// B-rep edges (none for a mesh body).
    pub fn edge_count(&self) -> usize {
        match &self.mesh {
            Some(_) => 0,
            None => Self::unique_edges(&self.solid).len(),
        }
    }
    /// B-rep vertices (none for a mesh body).
    pub fn vertex_count(&self) -> usize {
        if self.mesh.is_some() {
            return 0;
        }
        let mut seen = std::collections::HashSet::new();
        self.solid.vertex_iter().filter(|v| seen.insert(v.id())).count()
    }
    pub fn shell_count(&self) -> usize {
        match &self.mesh {
            Some(_) => 1,
            None => self.solid.boundaries().len(),
        }
    }
}
