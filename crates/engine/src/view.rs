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
    /// Fixed (locked) geometry.
    pub const SKETCH_LOCKED: Rgb = Rgb(0, 140, 60);
    pub const SKETCH_CENTERLINE: Rgb = Rgb(200, 110, 60);
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
    let st = s.world_state();
    let mut b = Aabb3::EMPTY;
    let v = &s.visibility;
    for body in st.bodies.iter().filter(|x| !v.hidden_bodies.contains(&x.name)) {
        b = b.union(&body.mesh().bounds());
    }
    for ss in st.sketches.iter().filter(|x| !v.hidden_sketches.contains(&x.feature)) {
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
        if c.link.is_some() && sk.view.hide_projected && !sk.is_text_curve(i) {
            continue;
        }
        let col = if c.link.is_some() && !sk.is_text_curve(i) {
            if sk.is_lost_curve(i) { colors::SKETCH_LOST } else { colors::SKETCH_PROJECTED }
        } else if c.fixed {
            colors::SKETCH_LOCKED
        } else if c.centerline {
            colors::SKETCH_CENTERLINE
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
            out.push((seg.polyline(0.02).iter().map(|p| plane.to_world(*p)).collect(), col, c.construction || c.centerline));
        }
    }
    // 3D curves are in world coordinates already.
    for w in sk.wires.iter().filter(|w| !w.fit.is_empty() || !sk.view.hide_projected) {
        // Curves drawn in a 3D sketch look like the sketch's own; included ones are references.
        let col = if w.fit.is_empty() {
            colors::SKETCH_PROJECTED
        } else if active {
            colors::SKETCH
        } else {
            colors::SKETCH_DONE
        };
        out.push((w.pts.clone(), col, false));
    }
    out
}

/// Should sketch point `i` be drawn? Text outlines' points are not (they are not picked or
/// constrained individually).
pub fn sketch_point_visible(sk: &Sketch, i: usize) -> bool {
    match sk.points.get(i).and_then(|p| p.link.as_deref()) {
        Some(l) => sk.link(l).is_none_or(|l| l.kind != solvecraft_sketch::LinkKind::Text),
        None => true,
    }
}

/// The scene for a headless render.
pub fn scene(s: &Session, cam: &Camera) -> Scene {
    let st = s.world_state();
    let mut sc = Scene { background: Some((colors::BG_TOP, colors::BG_BOTTOM)), clip: s.section.map(|(o, n)| (n, n.dot(o))), ..Default::default() };
    let b = bounds(s);
    sc.radius = (b.diagonal() * 0.5).max(cam.half_height()).max(10.0) + b.center().dist(cam.target);
    // Grid on the ground plane (XY with Z up, XZ with Y up).
    let (minor, major) = grid_step(cam.half_height());
    let ext = (cam.half_height() * 3.0 / major).ceil() * major;
    let tf = cam.to_frame(cam.target);
    let c = Vec3::new((tf.x / major).round() * major, (tf.y / major).round() * major, 0.0);
    let w = |p: Vec3| cam.from_frame(p);
    let n = ((ext / minor) as i64).clamp(1, 400);
    for i in -n..=n {
        let t = i as f64 * minor;
        let col = if (t / major).fract().abs() < 1e-9 { colors::GRID_MAJOR } else { colors::GRID };
        sc.lines.push(SceneLine {
            points: vec![w(c + Vec3::new(t, -ext, 0.0)), w(c + Vec3::new(t, ext, 0.0))],
            color: col,
            width: 0.6,
            on_top: false,
        });
        sc.lines.push(SceneLine {
            points: vec![w(c + Vec3::new(-ext, t, 0.0)), w(c + Vec3::new(ext, t, 0.0))],
            color: col,
            width: 0.6,
            on_top: false,
        });
    }
    let axis = cam.half_height() * 0.25;
    for (d, col) in [(Vec3::X, colors::AXIS_X), (Vec3::Y, colors::AXIS_Y), (Vec3::Z, colors::AXIS_Z)] {
        sc.lines.push(SceneLine { points: vec![Vec3::ZERO, d * axis], color: col, width: 1.6, on_top: false });
    }
    for body in st.bodies.iter().filter(|b| !s.visibility.hidden_bodies.contains(&b.name)) {
        let selected = s.selection.iter().any(|x| matches!(x, Sel::Body { name } if *name == body.name));
        let mesh = body.mesh();
        // The body's appearance (or imported colour) and faces with looks of their own, as in
        // the viewport.
        let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color =
            if selected { colors::BODY_SELECTED } else { s.doc.body_color(body).map_or(colors::BODY, |[r, g, b]| Rgb(to8(r), to8(g), to8(b))) };
        let face_colors = if selected {
            Vec::new()
        } else {
            s.doc.face_colors(body).into_iter().map(|(f, l)| (f as u32, Rgb(l.color[0], l.color[1], l.color[2]))).collect()
        };
        sc.meshes.push(SceneMesh { mesh: Arc::clone(&mesh), color, face_colors });
        for (i, e) in mesh.edges.iter().enumerate() {
            if mesh.seams.get(i).copied().unwrap_or(false) {
                continue;
            }
            sc.lines.push(SceneLine { points: e.clone(), color: colors::EDGE, width: 1.2, on_top: false });
        }
    }
    for ss in &st.sketches {
        let active = s.active_sketch == Some(ss.feature);
        if !sketch_shown(s, ss.feature) {
            continue;
        }
        for (pts, col, cons) in sketch_lines(&ss.sketch, &ss.plane, active, &ss.report.curve_determined) {
            sc.lines.push(SceneLine { points: pts, color: col, width: if cons { 1.0 } else { 1.6 }, on_top: active });
        }
    }
    sc
}

/// Is a sketch drawn? The active one is; others unless hidden. Finished sketches are hidden
/// once a feature uses them, like Fusion's default, unless shown one by one.
pub fn sketch_shown(s: &Session, id: u64) -> bool {
    let v = &s.visibility;
    s.active_sketch == Some(id) || (!v.hidden_sketches.contains(&id) && (v.shown_sketches.contains(&id) || !sketch_consumed(s, id)))
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

/// Construction planes of the timeline, as evaluated: (feature id, name, plane).
pub fn construction_planes(s: &Session) -> Vec<(u64, String, Plane)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solvecraft_doc::construct::ConstructGeom::Plane(p) => Some((id, name, p)),
            _ => None,
        })
        .collect()
}

/// Construction axes, as evaluated: (feature id, name, origin, unit direction).
pub fn construction_axes(s: &Session) -> Vec<(u64, String, Vec3, Vec3)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solvecraft_doc::construct::ConstructGeom::Axis { origin, dir } => Some((id, name, origin, dir)),
            _ => None,
        })
        .collect()
}

/// Construction points, as evaluated: (feature id, name, position).
pub fn construction_points(s: &Session) -> Vec<(u64, String, Vec3)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solvecraft_doc::construct::ConstructGeom::Point(p) => Some((id, name, p)),
            _ => None,
        })
        .collect()
}

/// Construction geometry of unsuppressed features, shown where its component is placed.
fn constructs(s: &Session) -> Vec<(u64, String, solvecraft_doc::construct::ConstructGeom)> {
    use solvecraft_doc::construct::ConstructGeom;
    let st = s.model.state();
    st.construct
        .iter()
        .filter_map(|c| {
            let f = s.doc.feature(c.feature).filter(|f| !f.suppressed)?;
            let m = s.doc.component_transform(f.component);
            if solvecraft_doc::is_identity(&m) {
                return Some((c.feature, c.name.clone(), c.geom.clone()));
            }
            let (pt, v) = (|p: Vec3| solvecraft_doc::apply_point(&m, p), |d: Vec3| solvecraft_doc::apply_vector(&m, d));
            let g = match &c.geom {
                ConstructGeom::Plane(p) => ConstructGeom::Plane(Plane::new(pt(p.origin), v(p.x), v(p.y)).unwrap_or(*p)),
                ConstructGeom::Axis { origin, dir } => ConstructGeom::Axis { origin: pt(*origin), dir: v(*dir) },
                ConstructGeom::Point(p) => ConstructGeom::Point(pt(*p)),
            };
            Some((c.feature, c.name.clone(), g))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The CPU renderer (snapshots) shows appearances as the viewport does: a body's colour and
    /// faces with looks of their own.
    #[test]
    fn snapshot_scene_carries_appearances() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("solid.cylinder", &json!({"diameter": 14, "height": 30, "base": [50, 15, 0], "operation": "new"})).unwrap();
        s.execute("appearance.assign", &json!({"bodies": ["Body2"], "color": [220, 60, 40]})).unwrap();
        s.execute("appearance.assign", &json!({"faces": [[20, 15, 20]], "body": "Body1", "color": [40, 170, 80]})).unwrap();
        let cam = home_camera(&s);
        let sc = scene(&s, &cam);
        assert!(sc.meshes.iter().any(|m| m.color == Rgb(220, 60, 40)));
        assert!(sc.meshes.iter().any(|m| m.face_colors.iter().any(|(_, c)| *c == Rgb(40, 170, 80))));
        // And the rendered picture has the green top.
        let img = solvecraft_render::render(&sc, &cam, 200, 140);
        assert!(img.rgba.chunks(4).any(|p| p[1] > 120 && p[0] < 90 && p[2] < 110), "no green pixels");
    }

    /// Curved surfaces get a dark silhouette line in snapshots where they turn away.
    #[test]
    fn snapshots_draw_silhouettes_on_curved_surfaces() {
        let mut s = Session::default();
        s.execute("solid.sphere", &json!({"diameter": 20})).unwrap();
        let cam = home_camera(&s);
        let img = solvecraft_render::render(&scene(&s, &cam), &cam, 400, 400);
        let dark = img.rgba.chunks(4).filter(|p| p[0] < 70 && p[1] < 70 && p[2] < 75).count();
        assert!(dark > 40, "silhouette pixels: {dark}");
    }

    /// A section shows in snapshots: the cut part is gone and the cut face is drawn.
    #[test]
    fn snapshots_show_the_section() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s);
        let whole = solvecraft_render::render(&scene(&s, &cam), &cam, 200, 140).rgba;
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        let sc = scene(&s, &cam);
        assert!(sc.clip.is_some());
        let cut = solvecraft_render::render(&sc, &cam, 200, 140).rgba;
        let changed = whole.chunks(4).zip(cut.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(changed > 500, "{changed} pixels changed");
    }
}
