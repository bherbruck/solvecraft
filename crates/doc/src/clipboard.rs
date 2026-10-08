//! Copies of features (timeline Copy / Paste): references to sketches and features inside the
//! copied set are moved to the copies.

use std::collections::BTreeMap;

use crate::{FeatureKind, PatternKind, PlaneRef};

fn plane_names(p: &mut PlaneRef, names: &BTreeMap<String, String>) {
    let mut p = p;
    for _ in 0..64 {
        match p {
            PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } => p = base,
            PlaneRef::Construction { name } => {
                if let Some(n) = names.get(name) {
                    *name = n.clone();
                }
                return;
            }
            _ => return,
        }
    }
}

impl FeatureKind {
    /// Point references to sketches (by id) and features (by name) at their copies.
    pub fn remap_refs(&mut self, sketches: &BTreeMap<u64, u64>, names: &BTreeMap<String, String>) {
        let sk = |id: &mut u64| {
            if let Some(n) = sketches.get(id) {
                *id = *n;
            }
        };
        let feats = |list: &mut Vec<String>| {
            for x in list.iter_mut() {
                if let Some(n) = names.get(x) {
                    *x = n.clone();
                }
            }
        };
        match self {
            FeatureKind::Sketch { plane, .. } | FeatureKind::ConstructionPlane { plane } | FeatureKind::Split { plane, .. } => {
                plane_names(plane, names)
            }
            FeatureKind::Extrude { sketch, .. }
            | FeatureKind::Revolve { sketch, .. }
            | FeatureKind::Emboss { sketch, .. }
            | FeatureKind::Rib { sketch, .. } => sk(sketch),
            FeatureKind::Sweep { sketch, path_sketch, .. } => {
                sk(sketch);
                sk(path_sketch);
            }
            FeatureKind::Pipe { path_sketch, .. } => sk(path_sketch),
            FeatureKind::Loft { sections, .. } => sections.iter_mut().for_each(|s| sk(&mut s.sketch)),
            FeatureKind::Hole { points, .. } => {
                if let Some(p) = points {
                    sk(&mut p.sketch);
                }
            }
            FeatureKind::Pattern { features, pattern, .. } => {
                feats(features);
                if let PatternKind::Path { path_sketch, .. } = pattern {
                    sk(path_sketch);
                }
            }
            FeatureKind::Mirror { features, plane, .. } => {
                feats(features);
                plane_names(plane, names);
            }
            FeatureKind::Draft { neutral, .. } => plane_names(neutral, names),
            FeatureKind::ReplaceFace { target, .. } => plane_names(target, names),
            _ => {}
        }
    }
}
