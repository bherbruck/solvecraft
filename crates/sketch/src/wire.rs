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
            self.links.push(Link { id: id.clone(), kind, source, lost: false });
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
            self.wires.push(Wire { id: wid, pts, link: Some(id.to_string()) });
        }
        Ok(())
    }

    /// Remove wires by id.
    pub fn remove_wires(&mut self, ids: &[String]) {
        self.wires.retain(|w| !ids.contains(&w.id));
        self.prune_links();
    }
}
