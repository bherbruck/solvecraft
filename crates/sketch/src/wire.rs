//! 3D sketch curves (wires): polylines in world coordinates that a sketch carries besides its
//! planar geometry — included 3D edges, curves projected onto surfaces, intersection curves.
//! They are reference geometry: drawn and usable as paths, not part of profiles or the solver.

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;

use crate::link::{Link, LinkGeom, LinkKind, LinkSource};
use crate::model::{Result, Sketch, SketchError};

/// Most points one wire may have.
pub const MAX_WIRE_POINTS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: String,
    /// World-space polyline.
    pub pts: Vec<Vec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Drawn in a 3D sketch: the points the curve was drawn through (one: a point; two: a
    /// line; more: a smooth spline). `pts` is made from them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fit: Vec<Vec3>,
}

/// Most fit points of a drawn 3D curve.
pub const MAX_FIT_POINTS: usize = 500;

/// Samples per span of a drawn 3D spline.
const SPAN_SAMPLES: usize = 24;

/// The polyline of a drawn 3D curve through `fit`: the point itself, the line, or a centripetal
/// Catmull–Rom spline through the points (smooth, no overshoot loops).
pub fn fit_polyline(fit: &[Vec3]) -> Vec<Vec3> {
    if fit.len() <= 2 {
        return fit.to_vec();
    }
    let n = fit.len();
    let at = |i: isize| -> Vec3 {
        match usize::try_from(i) {
            Ok(i) if i < n => fit[i],
            // Mirrored end tangents.
            Ok(_) => fit[n - 1] * 2.0 - fit[n - 2],
            Err(_) => fit[0] * 2.0 - fit[1],
        }
    };
    let mut out = vec![fit[0]];
    for k in 0..n - 1 {
        let k = k as isize;
        let (p0, p1, p2, p3) = (at(k - 1), at(k), at(k + 1), at(k + 2));
        let tj = |a: Vec3, b: Vec3| a.dist(b).sqrt().max(1e-9);
        let t0 = 0.0;
        let t1 = t0 + tj(p0, p1);
        let t2 = t1 + tj(p1, p2);
        let t3 = t2 + tj(p2, p3);
        for s in 1..=SPAN_SAMPLES {
            let t = t1 + (t2 - t1) * s as f64 / SPAN_SAMPLES as f64;
            let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
            let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
            let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            out.push(b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1)));
        }
    }
    out
}

fn clean(w: &[Vec3]) -> Option<Vec<Vec3>> {
    let mut out: Vec<Vec3> = Vec::new();
    for p in w.iter().take(MAX_WIRE_POINTS) {
        if !(p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9 && p.z.abs() < 1e9) {
            return None;
        }
        if out.last().is_none_or(|l| l.dist(*p) > 1e-9) {
            out.push(*p);
        }
    }
    (out.len() >= 2).then_some(out)
}

impl Sketch {
    pub fn wire_index(&self, id: &str) -> Option<usize> {
        self.wires.iter().position(|w| w.id == id)
    }

    /// Wires owned by a link, in creation order.
    pub fn link_wires(&self, id: &str) -> Vec<usize> {
        self.wires.iter().enumerate().filter(|(_, w)| w.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Add a link with planar geometry and/or 3D wires.
    pub fn add_link_with_wires(&mut self, kind: LinkKind, source: LinkSource, geom: &[LinkGeom], wires: &[Vec<Vec3>]) -> Result<String> {
        let ws: Vec<Vec<Vec3>> = wires.iter().filter_map(|w| clean(w)).collect();
        if ws.is_empty() {
            return self.add_link(kind, source, geom);
        }
        let id = if geom.is_empty() {
            let id = self.fresh("j");
            self.links.push(Link { id: id.clone(), kind, source, face_name: None, lost: false });
            id
        } else {
            self.add_link(kind, source, geom)?
        };
        self.set_link_wires(&id, &ws)?;
        Ok(id)
    }

    /// Replace a link's wires (ids are kept in order; extra wires get new ids).
    pub fn set_link_wires(&mut self, id: &str, wires: &[Vec<Vec3>]) -> Result<()> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        let ws: Vec<Vec<Vec3>> = wires.iter().filter_map(|w| clean(w)).collect();
        let old: Vec<String> = self.link_wires(id).iter().filter_map(|i| self.wires.get(*i).map(|w| w.id.clone())).collect();
        self.wires.retain(|w| w.link.as_deref() != Some(id));
        for (k, pts) in ws.into_iter().enumerate() {
            let wid = match old.get(k) {
                Some(o) => o.clone(),
                None => self.fresh("w"),
            };
            self.wires.push(Wire { id: wid, pts, link: Some(id.to_string()), fit: Vec::new() });
        }
        Ok(())
    }

    /// Add a curve drawn in a 3D sketch through `fit` (see [`Wire::fit`]).
    pub fn add_drawn_wire(&mut self, fit: &[Vec3]) -> Result<String> {
        if fit.is_empty() || fit.len() > MAX_FIT_POINTS {
            return Err(SketchError::Invalid(format!("a 3D curve needs 1 to {MAX_FIT_POINTS} points")));
        }
        if fit.iter().any(|p| !(p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9 && p.z.abs() < 1e9)) {
            return Err(SketchError::Invalid("3D point out of range".into()));
        }
        if fit.windows(2).any(|w| w[0].dist(w[1]) < 1e-9) {
            return Err(SketchError::Invalid("the points of a 3D curve must differ".into()));
        }
        let id = self.fresh(if fit.len() == 1 { "q" } else { "w" });
        self.wires.push(Wire { id: id.clone(), pts: fit_polyline(fit), link: None, fit: fit.to_vec() });
        Ok(id)
    }

    /// Move a fit point of a drawn 3D curve, and every other drawn point at the same place
    /// (curves drawn end to end stay joined). Returns how many points moved.
    pub fn move_drawn_point(&mut self, wire: &str, index: usize, to: Vec3) -> Result<usize> {
        if !(to.is_finite() && to.x.abs() < 1e9 && to.y.abs() < 1e9 && to.z.abs() < 1e9) {
            return Err(SketchError::Invalid("3D point out of range".into()));
        }
        let w = self.wire_index(wire).ok_or_else(|| SketchError::Unknown(wire.into()))?;
        let from = self.wires.get(w).and_then(|w| w.fit.get(index)).copied().ok_or_else(|| SketchError::Unknown(format!("{wire} point {index}")))?;
        let mut n = 0;
        for w in self.wires.iter_mut().filter(|w| !w.fit.is_empty()) {
            let mut changed = false;
            for p in w.fit.iter_mut().filter(|p| p.dist(from) < 1e-9) {
                *p = to;
                changed = true;
                n += 1;
            }
            if changed {
                w.pts = fit_polyline(&w.fit);
            }
        }
        Ok(n)
    }

    /// Remove wires by id.
    pub fn remove_wires(&mut self, ids: &[String]) {
        self.wires.retain(|w| !ids.contains(&w.id));
        self.prune_links();
    }
}
