//! Sewing a solid from face descriptions: each face is its boundary loops (lines and circular
//! arcs, counter-clockwise about the outward normal; holes clockwise) and its surface (planar,
//! revolved from a line, or extruded from an arc). Vertices and edges at the same places are
//! shared, so the faces close into one shell without booleans — for solids whose faces are
//! known exactly, such as folded sheet metal.

use std::collections::HashMap;

use mt::builder;
use solvecraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::{Body, Solid, p3, v3};
use crate::{KernelError, Result, guard};

/// A boundary edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeSpec {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// A circular arc from `a` to `b` through `mid`.
    Arc {
        a: Vec3,
        b: Vec3,
        mid: Vec3,
    },
}

impl EdgeSpec {
    fn ends(&self) -> (Vec3, Vec3) {
        match *self {
            EdgeSpec::Line { a, b } | EdgeSpec::Arc { a, b, .. } => (a, b),
        }
    }
}

/// A face's surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SurfSpec {
    /// The plane of the boundary.
    Plane,
    /// The surface the line `a`→`b` sweeps turning `angle` about the axis.
    Revolved { a: Vec3, b: Vec3, axis_origin: Vec3, axis: Vec3, angle: f64 },
    /// The surface an arc sweeps moving along `dir`.
    Extruded { a: Vec3, b: Vec3, mid: Vec3, dir: Vec3 },
}

/// A face: boundary loops, surface, and a point on it with its outward normal (curved faces).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceSpec {
    pub loops: Vec<Vec<EdgeSpec>>,
    pub surface: SurfSpec,
    pub probe: Option<(Vec3, Vec3)>,
}

type Key = (i64, i64, i64);

fn key(p: Vec3) -> Key {
    let q = |x: f64| (x * 1e6).round() as i64;
    (q(p.x), q(p.y), q(p.z))
}

#[derive(Default)]
struct Cache {
    verts: HashMap<Key, mt::Vertex>,
    edges: HashMap<(Key, Key, Option<Key>), mt::Edge>,
}

impl Cache {
    fn vertex(&mut self, p: Vec3) -> mt::Vertex {
        self.verts.entry(key(p)).or_insert_with(|| builder::vertex(p3(p))).clone()
    }
    fn edge(&mut self, e: &EdgeSpec) -> mt::Edge {
        let (a, b) = e.ends();
        let mid = match e {
            EdgeSpec::Arc { mid, .. } => Some(key(*mid)),
            EdgeSpec::Line { .. } => None,
        };
        let (ka, kb) = (key(a), key(b));
        if let Some(found) = self.edges.get(&(ka, kb, mid)) {
            return found.clone();
        }
        if let Some(found) = self.edges.get(&(kb, ka, mid)) {
            return found.inverse();
        }
        let (va, vb) = (self.vertex(a), self.vertex(b));
        let made = match *e {
            EdgeSpec::Line { .. } => builder::line(&va, &vb),
            EdgeSpec::Arc { mid, .. } => builder::circle_arc(&va, &vb, p3(mid)),
        };
        self.edges.insert((ka, kb, mid), made.clone());
        made
    }
}

/// Sew the faces into a closed solid.
pub fn sew(faces: &[FaceSpec]) -> Result<Body> {
    if faces.len() < 4 || faces.len() > 200_000 {
        return Err(KernelError::Invalid("a solid needs 4…200000 faces".into()));
    }
    for f in faces {
        for l in &f.loops {
            for e in l {
                let (a, b) = e.ends();
                if !a.is_finite() || !b.is_finite() || a.dist(b) < 1e-9 {
                    return Err(KernelError::Invalid("sew: a degenerate edge".into()));
                }
            }
        }
    }
    guard("sew", || {
        let mut cache = Cache::default();
        let mut out: Vec<mt::Face> = Vec::with_capacity(faces.len());
        for (fi, f) in faces.iter().enumerate() {
            let wires: Vec<mt::Wire> = f.loops.iter().map(|l| l.iter().map(|e| cache.edge(e)).collect::<Vec<_>>().into()).collect();
            let face = match f.surface {
                SurfSpec::Plane => builder::try_attach_plane(&wires).map_err(|e| KernelError::Failed(format!("sew: face {fi}: {e}")))?,
                SurfSpec::Revolved { a, b, axis_origin, axis, angle } => {
                    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("sew: axis".into()))?;
                    let (va, vb) = (builder::vertex(p3(a)), builder::vertex(p3(b)));
                    let swept: mt::Shell = builder::rsweep(&builder::line(&va, &vb), p3(axis_origin), v3(k), mt::Rad(angle));
                    let tmp = swept.face_iter().next().cloned().ok_or_else(|| KernelError::Failed("sew: revolved surface".into()))?;
                    curved(tmp.oriented_surface(), wires, f.probe, fi)?
                }
                SurfSpec::Extruded { a, b, mid, dir } => {
                    let (va, vb) = (builder::vertex(p3(a)), builder::vertex(p3(b)));
                    let tmp: mt::Face = builder::tsweep(&builder::circle_arc(&va, &vb, p3(mid)), v3(dir));
                    curved(tmp.oriented_surface(), wires, f.probe, fi)?
                }
            };
            out.push(face);
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Failed(format!("sew: the faces do not close: {e}")))?;
        Body::new(solid)
    })
}

/// A curved face on `surf`, turned so its normal at the probe points outward.
fn curved(mut surf: mt::Surface, wires: Vec<mt::Wire>, probe: Option<(Vec3, Vec3)>, fi: usize) -> Result<mt::Face> {
    use mt::{ParametricSurface3D, SearchParameter};
    if let Some((p, outward)) = probe
        && let Some((u, v)) = surf.search_parameter(p3(p), None, 100)
    {
        let n = surf.normal(u, v);
        if Vec3::new(n.x, n.y, n.z).dot(outward) < 0.0 {
            surf = mt::Invertible::inverse(&surf);
        }
    }
    mt::Face::try_new(wires, surf).map_err(|e| KernelError::Failed(format!("sew: face {fi}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> FaceSpec {
        FaceSpec {
            loops: vec![vec![EdgeSpec::Line { a, b }, EdgeSpec::Line { a: b, b: c }, EdgeSpec::Line { a: c, b: d }, EdgeSpec::Line { a: d, b: a }]],
            surface: SurfSpec::Plane,
            probe: None,
        }
    }

    #[test]
    fn a_sewn_box() {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let (a, b, c, d) = (p(0., 0., 0.), p(2., 0., 0.), p(2., 3., 0.), p(0., 3., 0.));
        let (e, f, g, h) = (p(0., 0., 4.), p(2., 0., 4.), p(2., 3., 4.), p(0., 3., 4.));
        let faces = vec![quad(a, d, c, b), quad(e, f, g, h), quad(a, b, f, e), quad(b, c, g, f), quad(c, d, h, g), quad(d, a, e, h)];
        let body = sew(&faces).unwrap();
        let m = crate::measure(&body).unwrap();
        assert!((m.volume - 24.0).abs() < 1e-9, "{}", m.volume);
        assert_eq!(body.face_count(), 6);
        assert!(sew(&faces[..5]).is_err());
    }
}
