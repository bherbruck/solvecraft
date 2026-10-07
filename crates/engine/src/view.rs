//! What the viewport shows, independent of the UI toolkit: body meshes, edges, sketches, the
//! grid and the origin. Used by the headless snapshot renderer; the GPU viewport draws the same
//! content.

use std::sync::Arc;

use solvecraft_geom::{Aabb3, Plane, Vec3};
use solvecraft_render::{Camera, Rgb, Scene, SceneLine, SceneMesh};
use solvecraft_sketch::Sketch;

use crate::{Sel, Session};

/// Colours (shared with the GPU viewport).
pub mod colors {
    use solvecraft_render::Rgb;
    pub const BODY: Rgb = Rgb(176, 186, 198);
    pub const BODY_SELECTED: Rgb = Rgb(110, 170, 235);
    pub const EDGE: Rgb = Rgb(40, 44, 52);
    pub const SKETCH: Rgb = Rgb(30, 90, 200);
    pub const SKETCH_DONE: Rgb = Rgb(20, 20, 20);
    pub const SKETCH_CONSTRUCTION: Rgb = Rgb(230, 130, 40);
    pub const SKETCH_FIXED: Rgb = Rgb(20, 20, 20);
    /// Linked (projected) reference geometry.
    pub const SKETCH_PROJECTED: Rgb = Rgb(150, 60, 190);
    /// Linked geometry whose reference was lost.
    pub const SKETCH_LOST: Rgb = Rgb(200, 60, 90);
    pub const PROFILE: Rgb = Rgb(255, 196, 120);
    pub const GRID: Rgb = Rgb(196, 202, 212);
    pub const GRID_MAJOR: Rgb = Rgb(170, 178, 190);
    pub const AXIS_X: Rgb = Rgb(220, 60, 60);
    pub const AXIS_Y: Rgb = Rgb(60, 170, 60);
    pub const AXIS_Z: Rgb = Rgb(60, 100, 230);
    pub const BG_TOP: Rgb = Rgb(240, 243, 247);
    pub const BG_BOTTOM: Rgb = Rgb(203, 210, 221);
}

/// Bounds of everything visible.
pub fn bounds(s: &Session) -> Aabb3 {
    let st = s.model.state();
    let mut b = Aabb3::EMPTY;
    for body in &st.bodies {
        b = b.union(&body.mesh().bounds());
    }
    for ss in &st.sketches {
        if let Some((lo, hi)) = ss.sketch.bounds() {
            for p in [lo, hi] {
                b.add(ss.plane.to_world(p));
            }
        }
    }
    b
}

/// Grid spacing (minor, major) for a view of the given half height.
pub fn grid_step(half_height: f64) -> (f64, f64) {
    let raw = (half_height / 10.0).max(1e-3);
    let p = 10f64.powf(raw.log10().floor());
    let minor = if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    };
    (minor, minor * 5.0)
}

/// Sketch curves as world polylines with their colour.
pub fn sketch_lines(sk: &Sketch, plane: &Plane, active: bool, determined: &[bool]) -> Vec<(Vec<Vec3>, Rgb, bool)> {
    let mut out = Vec::new();
    for (i, c) in sk.curves.iter().enumerate() {
        let col = if c.link.is_some() && !sk.is_text_curve(i) {
            if sk.is_lost_curve(i) { colors::SKETCH_LOST } else { colors::SKETCH_PROJECTED }
        } else if c.construction {
            colors::SKETCH_CONSTRUCTION
        } else if !active {
            colors::SKETCH_DONE
        } else if determined.get(i).copied().unwrap_or(false) {
            colors::SKETCH_FIXED
        } else {
            colors::SKETCH
        };
        for seg in sk.segs(i) {
            out.push((seg.polyline(0.02).iter().map(|p| plane.to_world(*p)).collect(), col, c.construction));
        }
    }
    out
}

/// The scene for a headless render.
pub fn scene(s: &Session, cam: &Camera) -> Scene {
    let st = s.model.state();
    let mut sc = Scene { background: Some((colors::BG_TOP, colors::BG_BOTTOM)), ..Default::default() };
    let b = bounds(s);
    sc.radius = (b.diagonal() * 0.5).max(cam.half_height()).max(10.0) + b.center().dist(cam.target);
    // Grid on XY.
    let (minor, major) = grid_step(cam.half_height());
    let ext = (cam.half_height() * 3.0 / major).ceil() * major;
    let c = Vec3::new((cam.target.x / major).round() * major, (cam.target.y / major).round() * major, 0.0);
    let n = ((ext / minor) as i64).clamp(1, 400);
    for i in -n..=n {
        let t = i as f64 * minor;
        let col = if (t / major).fract().abs() < 1e-9 { colors::GRID_MAJOR } else { colors::GRID };
        sc.lines.push(SceneLine { points: vec![c + Vec3::new(t, -ext, 0.0), c + Vec3::new(t, ext, 0.0)], color: col, width: 0.6, on_top: false });
        sc.lines.push(SceneLine { points: vec![c + Vec3::new(-ext, t, 0.0), c + Vec3::new(ext, t, 0.0)], color: col, width: 0.6, on_top: false });
    }
    let axis = cam.half_height() * 0.25;
    for (d, col) in [(Vec3::X, colors::AXIS_X), (Vec3::Y, colors::AXIS_Y), (Vec3::Z, colors::AXIS_Z)] {
        sc.lines.push(SceneLine { points: vec![Vec3::ZERO, d * axis], color: col, width: 1.6, on_top: false });
    }
    for body in &st.bodies {
        let selected = s.selection.iter().any(|x| matches!(x, Sel::Body { name } if *name == body.name));
        let mesh = body.mesh();
        sc.meshes.push(SceneMesh { mesh: Arc::clone(&mesh), color: if selected { colors::BODY_SELECTED } else { colors::BODY } });
        for (i, e) in mesh.edges.iter().enumerate() {
            if mesh.seams.get(i).copied().unwrap_or(false) {
                continue;
            }
            sc.lines.push(SceneLine { points: e.clone(), color: colors::EDGE, width: 1.2, on_top: false });
        }
    }
    for ss in &st.sketches {
        let active = s.active_sketch == Some(ss.feature);
        // Finished sketches are hidden once a feature uses them, like Fusion's default.
        if !active && sketch_consumed(s, ss.feature) {
            continue;
        }
        for (pts, col, cons) in sketch_lines(&ss.sketch, &ss.plane, active, &ss.report.curve_determined) {
            sc.lines.push(SceneLine { points: pts, color: col, width: if cons { 1.0 } else { 1.6 }, on_top: active });
        }
    }
    sc
}

/// Is a sketch used by a later feature?
pub fn sketch_consumed(s: &Session, id: u64) -> bool {
    use solvecraft_doc::FeatureKind;
    s.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } if sketch == id))
}

/// Camera framing the model (iso view).
pub fn home_camera(s: &Session) -> Camera {
    let mut c = Camera::default();
    let b = bounds(s);
    if !b.is_empty() {
        c.fit(&b);
    } else {
        c.distance = 150.0;
    }
    c
}

/// Construction planes of the timeline: (feature id, name, plane).
pub fn construction_planes(s: &Session) -> Vec<(u64, String, Plane)> {
    use solvecraft_doc::FeatureKind;
    let (vals, _) = s.doc.param_values();
    s.doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::ConstructionPlane { plane } if !f.suppressed => s.doc.resolve_plane(&vals, plane, 0).ok().map(|p| (f.id, f.name.clone(), p)),
            _ => None,
        })
        .collect()
}
