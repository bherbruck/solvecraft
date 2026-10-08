//! The 3D viewport: navigation, GPU (or CPU fallback) rendering, picking, overlays, the view
//! cube and the navigation bar.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use serde_json::json;
use solvecraft_engine::Sel;
use solvecraft_engine::geom::{Vec2, Vec3};
use solvecraft_engine::render::{Camera, Mat4, Rgb, StandardView};
use solvecraft_engine::sketch::{ConstraintKind, CurveKind};
use solvecraft_engine::view::{colors, grid_step, sketch_consumed, sketch_lines};
use std::hash::{Hash, Hasher};

use crate::gpu::{GpuScene, GpuTarget, SceneSlot, ViewportCallback};
use crate::selection::{BoxSel, origin_quads, origin_size, ray_quad};
use crate::theme::Tokens;
use crate::{SolveApp, icons};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
}

#[derive(Default)]
pub struct ViewportState {
    /// Per-body bounding boxes for fast picking, keyed by the model they were taken from.
    pick_bounds: std::sync::Mutex<Option<(usize, std::sync::Arc<Vec<(String, solvecraft_engine::geom::Aabb3)>>)>>,
    /// The last pick (its key and hits): reused while nothing it depends on changed.
    last_pick: Option<(u64, Vec<Hit>)>,
    pub rect: Option<Rect>,
    pub gpu: Option<GpuTarget>,
    slot: SceneSlot,
    key: u64,
    hl_slot: SceneSlot,
    hl_key: u64,
    cpu: Option<(u64, egui::TextureHandle)>,
    pub nav: Option<NavMode>,
    /// What a click would pick now (after the active command's filter).
    pub hover: Option<Hit>,
    pub mouse: Option<Pos2>,
    /// Box selection being dragged.
    pub boxsel: Option<BoxSel>,
    /// Timeline item under the cursor: its bodies are highlighted.
    pub hover_feature: Option<u64>,
    /// Bodies whose browser row is under the cursor: highlighted the same way.
    pub hover_bodies: Vec<String>,
    /// The model point an orbit turns around (Preferences: orbit around the cursor).
    pub orbit_pivot: Option<Vec3>,
    /// Continuous orbit: the view turns about the vertical until the next click or key.
    pub spin: bool,
    pub build_ms: f64,
    /// The right-click menu, open at this screen position.
    pub context_menu: Option<Pos2>,
    /// A sketch point or curve being dragged (its id, the undo depth when the drag began: the
    /// whole drag is one undo step, and for a curve where it was last under the cursor).
    pub point_drag: Option<(String, usize, Option<Vec2>)>,
}

/// Something under the cursor.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    SketchPoint {
        sketch: u64,
        id: String,
        at: Vec2,
    },
    /// A sketch curve; `straight` for lines (usable as an axis).
    SketchCurve {
        sketch: u64,
        id: String,
        straight: bool,
    },
    Vertex {
        body: String,
        point: Vec3,
    },
    Edge {
        body: String,
        index: usize,
        mid: Vec3,
    },
    Profile {
        sketch: u64,
        index: usize,
    },
    Face {
        body: String,
        index: usize,
        point: Vec3,
    },
    /// An origin plane (XY, XZ, YZ) or a construction plane (by name).
    Plane {
        name: String,
        point: Vec3,
    },
    /// An origin axis (X, Y, Z).
    Axis {
        name: String,
    },
}

/// World ↔ screen mapping for the current frame.
#[derive(Clone, Copy)]
pub struct Proj {
    pub cam: Camera,
    pub vp: Mat4,
    pub rect: Rect,
}

impl Proj {
    pub fn to_screen(&self, p: Vec3) -> Option<Pos2> {
        let c = self.vp.apply(p);
        if c[3] <= 1e-9 {
            return None;
        }
        let (x, y) = (c[0] / c[3], c[1] / c[3]);
        Some(pos2(
            self.rect.left() + ((x + 1.0) * 0.5 * self.rect.width() as f64) as f32,
            self.rect.top() + ((1.0 - y) * 0.5 * self.rect.height() as f64) as f32,
        ))
    }
    pub fn ray(&self, p: Pos2) -> (Vec3, Vec3) {
        self.cam.ray((p.x - self.rect.left()) as f64, (p.y - self.rect.top()) as f64, self.rect.width() as f64, self.rect.height() as f64)
    }
}

fn rgba(c: Rgb) -> [u8; 4] {
    [c.0, c.1, c.2, 255]
}

pub fn scene_radius(app: &SolveApp) -> f64 {
    let mut b = model_bounds(app);
    if !app.preview.bounds.is_empty() {
        b = b.union(&app.preview.bounds);
    }
    let r = if b.is_empty() { 50.0 } else { b.diagonal() * 0.5 + b.center().dist(app.cam.target) };
    r.max(app.cam.half_height() * 2.0).max(10.0)
}

pub fn projection(app: &SolveApp, rect: Rect) -> Proj {
    let mut cam = app.cam;
    cam.fov = if app.ui.perspective { 0.6 } else { 0.0 };
    let aspect = rect.width() as f64 / rect.height().max(1.0) as f64;
    Proj { cam, vp: cam.view_proj(aspect, scene_radius(app)), rect }
}

/// Sketches drawn in the viewport: the active one, and finished ones not yet used by a feature.
fn visible_sketches(app: &SolveApp) -> Vec<u64> {
    let st = app.session.model.state();
    st.sketches
        .iter()
        .filter(|s| {
            app.session.active_sketch == Some(s.feature)
                || (!app.ui.hidden_sketches.contains(&s.feature)
                    && (app.ui.shown_sketches.contains(&s.feature) || (app.ui.show_sketches && !sketch_consumed(&app.session, s.feature))))
        })
        .map(|s| s.feature)
        .collect()
}

fn scene_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.session.revision.hash(&mut h);
    app.ui.show_grid.hash(&mut h);
    app.ui.visual_style.hash(&mut h);
    app.ui.ground_shadow.hash(&mut h);
    (app.cam.pitch > 0.0).hash(&mut h);
    app.ui.show_sketches.hash(&mut h);
    app.ui.hidden_bodies.hash(&mut h);
    app.ui.hidden_sketches.hash(&mut h);
    app.ui.shown_sketches.hash(&mut h);
    app.ui.hidden_profiles.hash(&mut h);
    app.session.active_component.hash(&mut h);
    app.preview.replaced.hash(&mut h);
    crate::theme::is_dark().hash(&mut h);
    app.session.active_sketch.hash(&mut h);
    let (minor, _) = grid_step(app.cam.half_height());
    minor.to_bits().hash(&mut h);
    ((app.cam.target.x / (minor * 5.0)).round() as i64).hash(&mut h);
    ((app.cam.target.y / (minor * 5.0)).round() as i64).hash(&mut h);
    h.finish()
}

/// A sketch curve colour from the engine, in the current theme.
fn sketch_color(t: &Tokens, c: Rgb) -> [u8; 4] {
    if c == colors::SKETCH {
        c4(t.sketch)
    } else if c == colors::SKETCH_DONE || c == colors::SKETCH_FIXED {
        c4(t.sketch_fixed)
    } else if c == colors::SKETCH_LOCKED {
        c4(t.sketch_locked)
    } else {
        rgba(c)
    }
}

fn build_scene(app: &SolveApp) -> GpuScene {
    let tk = Tokens::get();
    let mut sc = GpuScene::default();
    let s = &app.session;
    // Bodies where their occurrences place them.
    let st = s.world_state();
    if app.ui.show_grid && !active_view(app).hide_grid {
        let (minor, major) = grid_step(app.cam.half_height());
        let ext = (app.cam.half_height() * 3.0 / major).ceil() * major;
        let c = Vec3::new((app.cam.target.x / major).round() * major, (app.cam.target.y / major).round() * major, 0.0);
        let n = ((ext / minor) as i64).clamp(1, 300);
        for i in -n..=n {
            let t = i as f64 * minor;
            let maj = (t / major).round() * major == t;
            let col = if maj { c4(tk.grid_major) } else { c4(tk.grid) };
            let w = if maj { 1.0 } else { 0.7 };
            sc.under_line((c + Vec3::new(t, -ext, 0.0)).to_f32(), (c + Vec3::new(t, ext, 0.0)).to_f32(), col, w);
            sc.under_line((c + Vec3::new(-ext, t, 0.0)).to_f32(), (c + Vec3::new(ext, t, 0.0)).to_f32(), col, w);
        }
    }
    for b in &st.bodies {
        if app.ui.hidden_bodies.contains(&b.name) || app.preview.replaced.contains(&b.name) {
            continue;
        }
        // With a component active, bodies outside it are drawn faded.
        let faded = s.active_component != 0 && !s.doc.component_within(s.doc.body_component(&b.name, b.feature), s.active_component);
        let base = body_color(&s.doc, b).unwrap_or(tk.body);
        let col = if faded { c4(base.gamma_multiply(0.35)) } else { c4(base) };
        let m = b.mesh();
        let (shaded, edges) = match app.ui.visual_style {
            1 => (true, false),
            2 => (false, true),
            _ => (true, true),
        };
        let edge_col = if shaded { c4(tk.body_edge) } else { c4(tk.text) };
        // Shaded with hidden edges: edges behind the model show faintly.
        let hidden_edges = app.ui.visual_style == 3;
        let hidden_col = c4(tk.text.gamma_multiply(0.35));
        // Appearances: the body's opacity, and faces with looks of their own.
        let opacity = s.doc.body_look(&b.name, b.feature).map_or(1.0, |l| l.opacity);
        let face_looks: std::collections::HashMap<usize, solvecraft_engine::doc::appearance::Look> =
            if shaded { s.doc.face_colors(b).into_iter().collect() } else { Default::default() };
        for (ti, t) in m.triangles.iter().enumerate().filter(|_| shaded) {
            let (mut col, a) = match m.tri_face.get(ti).and_then(|f| face_looks.get(&(*f as usize))) {
                Some(l) => {
                    let c = Color32::from_rgb(l.color[0], l.color[1], l.color[2]);
                    (if faded { c4(c.gamma_multiply(0.35)) } else { c4(c) }, l.opacity)
                }
                None => (col, opacity),
            };
            let glass = !faded && a < 0.999;
            if glass {
                col[3] = (a.clamp(0.05, 1.0) * 255.0).round() as u8;
            }
            for k in t {
                let i = *k as usize;
                if let (Some(p), Some(n)) = (m.positions.get(i), m.normals.get(i)) {
                    if faded {
                        sc.trans_tri(p.to_f32(), n.to_f32(), col);
                    } else if glass {
                        sc.glass_tri(p.to_f32(), n.to_f32(), col);
                    } else {
                        sc.tri(p.to_f32(), n.to_f32(), col);
                    }
                }
            }
        }
        for (ei, e) in m.edges.iter().enumerate().filter(|_| edges) {
            if m.seams.get(ei).copied().unwrap_or(false) {
                continue;
            }
            for w in e.windows(2) {
                sc.line(w[0].to_f32(), w[1].to_f32(), edge_col, 1.3, false);
                if hidden_edges {
                    sc.hidden_line(w[0].to_f32(), w[1].to_f32(), hidden_col, 1.0);
                }
            }
        }
    }
    if app.ui.ground_shadow && app.ui.visual_style != 2 && app.cam.pitch > 0.0 {
        ground_shadow(&mut sc, app, &st);
    }
    for sid in visible_sketches(app) {
        let Some(ss) = st.sketch(sid) else { continue };
        let active = s.active_sketch == Some(sid);
        // Closed profiles are shaded (lifted off a face they may lie on).
        let lift = ss.plane.normal() * 0.01;
        let profiles = if app.ui.hidden_profiles.contains(&sid) || ss.sketch.view.hide_profile { &[][..] } else { &ss.profiles[..] };
        for p in profiles {
            for tri in p.region.triangulate(0.05) {
                for q in tri {
                    sc.trans_tri((ss.plane.to_world(q) + lift).to_f32(), [0.0; 3], c4(tk.profile_fill));
                }
            }
        }
        // Construction curves are dashed, centerlines dash-dot (lengths follow the grid).
        let unit = grid_step(app.cam.half_height()).0;
        for (pts, col, cons) in sketch_lines(&ss.sketch, &ss.plane, active, &ss.report.curve_determined) {
            let segs = if !cons {
                pts.windows(2).map(|w| (w[0], w[1])).collect()
            } else if col == colors::SKETCH_CENTERLINE {
                dashes(&pts, &[0.6 * unit, 0.15 * unit, 0.05 * unit, 0.15 * unit])
            } else {
                dashes(&pts, &[0.3 * unit, 0.2 * unit])
            };
            for (a, b) in segs {
                sc.line(a.to_f32(), b.to_f32(), sketch_color(&tk, col), if cons { 1.2 } else { 2.0 }, active);
            }
        }
    }
    sc
}

/// A soft dark ellipse on the ground under the shown bodies (darkest in the middle).
fn ground_shadow(sc: &mut GpuScene, app: &SolveApp, st: &solvecraft_engine::doc::ModelState) {
    let mut b = solvecraft_engine::geom::Aabb3::EMPTY;
    for x in st.bodies.iter().filter(|x| !app.ui.hidden_bodies.contains(&x.name)) {
        b = b.union(&x.mesh().bounds());
    }
    if b.is_empty() {
        return;
    }
    let size = b.size();
    let c = b.center();
    let z = b.min.z - 1e-3 * (1.0 + b.diagonal());
    let (rx, ry) = (size.x * 0.62 + size.z * 0.15, size.y * 0.62 + size.z * 0.15);
    if !(rx > 0.0 && ry > 0.0) {
        return;
    }
    let dark = if crate::theme::is_dark() { 150 } else { 80 };
    // Rings from the middle out, fading to nothing.
    let rings = [(0.0, dark), (0.55, dark * 3 / 4), (0.85, dark / 3), (1.0, 0)];
    let n = 48;
    let at = |r: f64, k: usize| {
        let a = k as f64 / n as f64 * std::f64::consts::TAU;
        Vec3::new(c.x + rx * r * a.cos(), c.y + ry * r * a.sin(), z).to_f32()
    };
    for w in rings.windows(2) {
        let ((r0, a0), (r1, a1)) = (w[0], w[1]);
        let (c0, c1) = ([0, 0, 0, a0 as u8], [0, 0, 0, a1 as u8]);
        for k in 0..n {
            let (p00, p01, p10, p11) = (at(r0, k), at(r0, k + 1), at(r1, k), at(r1, k + 1));
            sc.trans_tri(p00, [0.0; 3], c0);
            sc.trans_tri(p10, [0.0; 3], c1);
            sc.trans_tri(p11, [0.0; 3], c1);
            if r0 > 0.0 {
                sc.trans_tri(p00, [0.0; 3], c0);
                sc.trans_tri(p11, [0.0; 3], c1);
                sc.trans_tri(p01, [0.0; 3], c0);
            }
        }
    }
}

/// The drawn pieces of a polyline in a dash pattern (on, off, on, off… lengths).
pub(crate) fn dashes(pts: &[Vec3], pattern: &[f64]) -> Vec<(Vec3, Vec3)> {
    let mut out = Vec::new();
    if pattern.is_empty() || pattern.iter().any(|l| !(l.is_finite() && *l > 0.0)) {
        return pts.windows(2).map(|w| (w[0], w[1])).collect();
    }
    let (mut k, mut left) = (0usize, pattern[0]);
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = (b - a).len();
        // Cap the pieces per segment so a huge zoom-out can't explode the scene.
        let mut guard = 0;
        while len > 1e-12 && guard < 10_000 {
            guard += 1;
            let step = left.min(len);
            let q = a + (b - a) * (step / len);
            if k % 2 == 0 {
                out.push((a, q));
            }
            left -= step;
            len -= step;
            a = q;
            if left <= 1e-12 {
                k = (k + 1) % pattern.len();
                left = pattern.get(k).copied().unwrap_or(1.0);
            }
        }
    }
    out
}

/// Colour as straight (unpremultiplied) sRGBA bytes for the GPU.
fn c4(c: Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

/// Origin planes and axes currently shown, sized for the view: (name, normal, quad).
pub fn origin_planes(app: &SolveApp) -> Vec<(&'static str, Vec3, [Vec3; 4])> {
    if !app.origin_visible() {
        return Vec::new();
    }
    let creating = app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Sketch));
    origin_quads(origin_size(app.cam.half_height()))
        .into_iter()
        .filter(|(n, _, _)| creating || !app.ui.hidden_origin.iter().any(|h| h == n))
        .collect()
}

pub fn origin_axes(app: &SolveApp) -> Vec<(&'static str, Vec3)> {
    if !app.origin_visible() {
        return Vec::new();
    }
    [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)].into_iter().filter(|(n, _)| !app.ui.hidden_origin.iter().any(|h| h == n)).collect()
}

/// Construction planes drawn as squares around their origin: (name, normal, quad).
pub fn construction_quads(app: &SolveApp) -> Vec<(String, Vec3, [Vec3; 4])> {
    let h = origin_size(app.cam.half_height()) * 1.2;
    solvecraft_engine::view::construction_planes(&app.session)
        .into_iter()
        .filter(|(_, n, _)| !app.ui.hidden_origin.contains(n))
        .map(|(_, name, pl)| {
            let (o, x, y) = (pl.origin, pl.x * h, pl.y * h);
            (name, pl.normal(), [o - x - y, o + x - y, o + x + y, o - x + y])
        })
        .collect()
}

/// Pick what is under `pos`: sketch points, sketch curves, vertices, edges and axes first, then
/// the surfaces along the ray (profiles, faces, planes) nearest first.
/// Bounding boxes of the shown model's bodies (cached until the model changes).
fn body_bounds(
    app: &SolveApp,
    st: &std::sync::Arc<solvecraft_engine::doc::ModelState>,
) -> std::sync::Arc<Vec<(String, solvecraft_engine::geom::Aabb3)>> {
    let key = std::sync::Arc::as_ptr(st) as usize;
    if let Ok(mut g) = app.viewport.pick_bounds.lock() {
        if let Some((k, b)) = g.as_ref()
            && *k == key
        {
            return b.clone();
        }
        let b = std::sync::Arc::new(st.bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect::<Vec<_>>());
        *g = Some((key, b.clone()));
        return b;
    }
    std::sync::Arc::new(Vec::new())
}

/// The model's extent: bodies (from the cached boxes) and sketches.
pub fn model_bounds(app: &SolveApp) -> solvecraft_engine::geom::Aabb3 {
    let st = app.session.world_state();
    let mut b = body_bounds(app, &st).iter().fold(solvecraft_engine::geom::Aabb3::EMPTY, |a, (_, x)| a.union(x));
    for ss in &st.sketches {
        if let Some((lo, hi)) = ss.sketch.bounds() {
            for p in [lo, hi] {
                b.add(ss.plane.to_world(p));
            }
        }
    }
    b
}

/// Where a ray enters a box (None: it misses).
fn ray_box(o: Vec3, d: Vec3, b: &solvecraft_engine::geom::Aabb3, pad: f64) -> Option<f64> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (oo, dd, lo, hi) in [(o.x, d.x, b.min.x, b.max.x), (o.y, d.y, b.min.y, b.max.y), (o.z, d.z, b.min.z, b.max.z)] {
        let (lo, hi) = (lo - pad, hi + pad);
        if dd.abs() < 1e-15 {
            if oo < lo || oo > hi {
                return None;
            }
        } else {
            let (a, c) = ((lo - oo) / dd, (hi - oo) / dd);
            t0 = t0.max(a.min(c));
            t1 = t1.min(a.max(c));
        }
    }
    (t0 <= t1).then_some(t0)
}

/// A box's outline on screen, grown by `px` (None: behind the eye).
fn screen_box(proj: &Proj, b: &solvecraft_engine::geom::Aabb3, px: f32) -> Option<Rect> {
    let mut r = Rect::NOTHING;
    for i in 0..8 {
        let p = Vec3::new(
            if i & 1 == 0 { b.min.x } else { b.max.x },
            if i & 2 == 0 { b.min.y } else { b.max.y },
            if i & 4 == 0 { b.min.z } else { b.max.z },
        );
        r.extend_with(proj.to_screen(p)?);
    }
    Some(r.expand(px))
}

/// What is under `pos`, reusing the last answer while the cursor, the camera and the model are
/// unchanged.
fn pick_cached(app: &mut SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    pos.x.to_bits().hash(&mut h);
    pos.y.to_bits().hash(&mut h);
    for v in [app.cam.yaw, app.cam.pitch, app.cam.distance, app.cam.target.x, app.cam.target.y, app.cam.target.z] {
        v.to_bits().hash(&mut h);
    }
    app.session.revision.hash(&mut h);
    (std::sync::Arc::as_ptr(&app.session.world_state()) as usize).hash(&mut h);
    app.session.active_sketch.hash(&mut h);
    app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept).hash(&mut h);
    app.ui.hidden_bodies.hash(&mut h);
    app.ui.perspective.hash(&mut h);
    (proj.rect.width().to_bits(), proj.rect.height().to_bits()).hash(&mut h);
    let key = h.finish();
    if let Some((k, hits)) = &app.viewport.last_pick
        && *k == key
    {
        return hits.clone();
    }
    let hits = pick(app, proj, pos);
    app.viewport.last_pick = Some((key, hits.clone()));
    hits
}

pub fn pick(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    let s = &app.session;
    let st = s.world_state();
    let bounds = body_bounds(app, &st);
    let bound = |name: &str| bounds.iter().find(|(n, _)| n == name).map(|(_, b)| *b);
    let mut hits = Vec::new();
    let (o, d) = proj.ray(pos);
    // Sketch points of the active sketch (of every shown sketch while a command takes points).
    let wants_points = app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::POINTS != 0);
    let point_sketches: Vec<u64> = if wants_points { visible_sketches(app) } else { s.active_sketch.into_iter().collect() };
    let mut best: Option<(f32, Hit)> = None;
    for ss in point_sketches.iter().filter_map(|sid| st.sketch(*sid)) {
        let sid = ss.feature;
        for p in &ss.sketch.points {
            if let Some(sp) = proj.to_screen(ss.plane.to_world(p.pos)) {
                let dd = sp.distance(pos);
                if dd < 8.0 && best.as_ref().is_none_or(|(b, _)| dd < *b) {
                    best = Some((dd, Hit::SketchPoint { sketch: sid, id: p.id.clone(), at: p.pos }));
                }
            }
        }
    }
    if let Some((_, h)) = best {
        hits.push(h);
    }
    // Sketch curves (the active sketch wins ties).
    let mut bestc: Option<(f32, Hit)> = None;
    for sid in visible_sketches(app) {
        let Some(ss) = st.sketch(sid) else { continue };
        let bonus = if s.active_sketch == Some(sid) { 2.0 } else { 0.0 };
        for (i, c) in ss.sketch.curves.iter().enumerate() {
            if c.link.is_some() && ss.sketch.view.hide_projected {
                continue;
            }
            let straight = matches!(c.kind, CurveKind::Line { .. });
            for seg in ss.sketch.segs(i) {
                let pts: Vec<Pos2> = seg.polyline(0.05).iter().filter_map(|q| proj.to_screen(ss.plane.to_world(*q))).collect();
                for w in pts.windows(2) {
                    let dd = seg_dist(pos, w[0], w[1]) - bonus;
                    if dd < 6.0 && bestc.as_ref().is_none_or(|(b, _)| dd < *b) {
                        bestc = Some((dd, Hit::SketchCurve { sketch: sid, id: c.id.clone(), straight }));
                    }
                }
            }
        }
        // 3D sketch curves (paths for sweeps and pipes).
        for w in &ss.sketch.wires {
            let pts: Vec<Pos2> = w.pts.iter().filter_map(|q| proj.to_screen(*q)).collect();
            for seg in pts.windows(2) {
                let dd = seg_dist(pos, seg[0], seg[1]) - bonus;
                if dd < 6.0 && bestc.as_ref().is_none_or(|(b, _)| dd < *b) {
                    bestc = Some((dd, Hit::SketchCurve { sketch: sid, id: w.id.clone(), straight: false }));
                }
            }
        }
    }
    if let Some((_, h)) = bestc {
        hits.push(h);
    }
    // Nearest face along the ray (hides what is behind it).
    let mut bestf: Option<(f64, Hit)> = None;
    for b in &st.bodies {
        if app.ui.hidden_bodies.contains(&b.name) {
            continue;
        }
        // Bodies whose box the ray misses (or enters behind the best hit so far) are skipped.
        if let Some(bb) = bound(&b.name) {
            let pad = bb.diagonal() * 1e-6 + 1e-9;
            match ray_box(o, d, &bb, pad) {
                None => continue,
                Some(t0) if bestf.as_ref().is_some_and(|(bt, _)| t0 > *bt) => continue,
                _ => {}
            }
        }
        let m = b.mesh();
        if let Some((t, ti)) = m.raycast(o, d)
            && bestf.as_ref().is_none_or(|(bt, _)| t < *bt)
        {
            let fi = m.tri_face.get(ti).copied().unwrap_or(0) as usize;
            bestf = Some((t, Hit::Face { body: b.name.clone(), index: fi, point: o + d * t }));
        }
    }
    let slack = scene_radius(app) * 0.02;
    let visible = |p: Vec3| bestf.as_ref().is_none_or(|(t, _)| (p - o).dot(d) <= *t + slack);
    // Vertices (edge end points) and edges.
    let mut bestv: Option<(f32, Hit)> = None;
    let mut beste: Option<(f32, Hit)> = None;
    for b in &st.bodies {
        if app.ui.hidden_bodies.contains(&b.name) {
            continue;
        }
        // Only bodies whose outline on screen is near the cursor can have an edge under it.
        if let Some(bb) = bound(&b.name)
            && screen_box(proj, &bb, 8.0).is_some_and(|r| !r.contains(pos))
        {
            continue;
        }
        let m = b.mesh();
        for (ei, e) in m.edges.iter().enumerate() {
            if m.seams.get(ei).copied().unwrap_or(false) {
                continue;
            }
            for v in [e.first(), e.last()].into_iter().flatten() {
                if let Some(sp) = proj.to_screen(*v) {
                    let dd = sp.distance(pos);
                    if dd < 7.0 && visible(*v) && bestv.as_ref().is_none_or(|(bd, _)| dd < *bd) {
                        bestv = Some((dd, Hit::Vertex { body: b.name.clone(), point: *v }));
                    }
                }
            }
            let pts: Vec<Pos2> = e.iter().filter_map(|q| proj.to_screen(*q)).collect();
            for (k, w) in pts.windows(2).enumerate() {
                let dd = seg_dist(pos, w[0], w[1]);
                if dd < 6.0 && beste.as_ref().is_none_or(|(bd, _)| dd < *bd) {
                    let mid = polyline_mid(e);
                    // Visible where the cursor is (not just at the middle).
                    let at = e.get(k).copied().unwrap_or(mid);
                    if visible(at) || visible(mid) {
                        beste = Some((dd, Hit::Edge { body: b.name.clone(), index: ei, mid }));
                    }
                }
            }
        }
    }
    hits.extend(bestv.map(|x| x.1));
    hits.extend(beste.map(|x| x.1));
    // Origin axes.
    let size = origin_size(app.cam.half_height());
    let mut besta: Option<(f32, Hit)> = None;
    for (name, dir) in origin_axes(app) {
        if let (Some(a), Some(b)) = (proj.to_screen(Vec3::ZERO), proj.to_screen(dir * (size * AXIS_LEN))) {
            let dd = seg_dist(pos, a, b);
            if dd < 5.0 && besta.as_ref().is_none_or(|(bd, _)| dd < *bd) {
                besta = Some((dd, Hit::Axis { name: name.into() }));
            }
        }
    }
    hits.extend(besta.map(|x| x.1));
    // Surfaces by depth: profiles (slightly preferred over a face they lie on), faces, planes.
    let mut surf: Vec<(f64, Hit)> = Vec::new();
    let mut bestp: Option<(f64, f64, Hit)> = None;
    for sid in visible_sketches(app) {
        let Some(ss) = st.sketch(sid) else { continue };
        let n = ss.plane.normal();
        let den = n.dot(d);
        if den.abs() < 1e-12 {
            continue;
        }
        let t = n.dot(ss.plane.origin - o) / den;
        if t < 0.0 {
            continue;
        }
        let lp = ss.plane.to_local(o + d * t);
        // Show Profile off: the sketch being edited offers no profiles.
        let profiles = if ss.sketch.view.hide_profile && s.active_sketch == Some(sid) { &[][..] } else { &ss.profiles[..] };
        for (pi, p) in profiles.iter().enumerate() {
            if p.region.contains(lp) && bestp.as_ref().is_none_or(|(_, a, _)| p.area < *a) {
                bestp = Some((t, p.area, Hit::Profile { sketch: sid, index: pi }));
            }
        }
    }
    if let Some((t, _, h)) = bestp {
        surf.push((t - slack, h));
    }
    if let Some(f) = bestf {
        surf.push(f);
    }
    for (name, _, q) in origin_planes(app) {
        if let Some(t) = ray_quad(o, d, &q) {
            surf.push((t, Hit::Plane { name: name.into(), point: o + d * t }));
        }
    }
    for (name, _, q) in construction_quads(app) {
        if let Some(t) = ray_quad(o, d, &q) {
            surf.push((t, Hit::Plane { name, point: o + d * t }));
        }
    }
    surf.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.extend(surf.into_iter().map(|x| x.1));
    hits
}

/// The section cut shown now: the open Section Analysis dialog's plane, else the session's.
/// The active sketch's display options (defaults when not sketching).
pub fn active_view(app: &SolveApp) -> solvecraft_engine::sketch::SketchView {
    let st = app.session.model.state();
    app.session.active_sketch.and_then(|s| st.sketch(s)).map(|ss| ss.sketch.view.clone()).unwrap_or_default()
}

pub fn section_plane(app: &SolveApp) -> Option<(Vec3, Vec3)> {
    if let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, crate::dialogs::Kind::Section { .. })) {
        let cmds = crate::dialogs::apply_commands(app, d).ok()?;
        let (_, p) = cmds.into_iter().next()?;
        let mut scratch = app.session.scratch();
        scratch.execute("FusionHalfSectionViewCommand", &p).ok()?;
        return scratch.section;
    }
    // Slice (Sketch Palette): cut the model at the plane of the sketch being edited.
    if app.session.section.is_none() {
        let st = app.session.model.state();
        if let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)).filter(|ss| ss.sketch.view.slice) {
            return Some((ss.plane.origin, ss.plane.normal()));
        }
    }
    app.session.section
}

/// The box around what is selected (bodies, faces, edges, vertices, sketch curves and the
/// bodies of selected features), where it is shown.
pub fn selection_bounds(app: &SolveApp) -> Option<solvecraft_engine::geom::Aabb3> {
    let st = app.session.world_state();
    let mut b = solvecraft_engine::geom::Aabb3::EMPTY;
    for s in &app.session.selection {
        match s {
            Sel::Body { name } => {
                if let Some(x) = st.body(name) {
                    b = b.union(&x.mesh().bounds());
                }
            }
            Sel::Face { body, index, .. } => {
                if let Some(x) = st.body(body) {
                    let m = x.mesh();
                    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                        if *f as usize == *index {
                            for p in m.tri(t).into_iter().flatten() {
                                b.add(p);
                            }
                        }
                    }
                }
            }
            Sel::Edge { body, index, .. } => {
                if let Some(e) = st.body(body).and_then(|x| x.mesh().edges.get(*index).cloned()) {
                    for p in e {
                        b.add(p);
                    }
                }
            }
            Sel::Vertex { point, .. } => b.add(*point),
            Sel::Feature { id } => {
                for x in st.bodies.iter().filter(|x| x.feature == *id) {
                    b = b.union(&x.mesh().bounds());
                }
            }
            Sel::SketchCurve { id } => {
                for ss in &st.sketches {
                    if let Some(i) = ss.sketch.curve_index(id) {
                        for q in ss.sketch.polyline(i) {
                            b.add(ss.plane.to_world(q));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (!b.is_empty()).then_some(b)
}

/// The section as the shader's clip plane (normal, d), zero when there is none.
/// The surface analysis for the shader (see `gpu::ViewportCallback::analysis`).
fn analysis_uniform(app: &SolveApp) -> [f32; 8] {
    use solvecraft_engine::SurfaceAnalysis as A;
    match app.session.analysis {
        Some(A::Zebra { stripes }) => [1.0, stripes as f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Draft { pull, angle }) => [2.0, angle as f32, 0.0, 0.0, pull.x as f32, pull.y as f32, pull.z as f32, 0.0],
        Some(A::Curvature { radius }) => [3.0, radius as f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Environment) => [4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Access { dir }) => {
            let (scale, size, eps) = crate::access::map(app).map(|m| (m.scale, m.size as f32, 1.5 / m.scale.max(1e-9))).unwrap_or((0.0, 0.0, 0.0));
            [5.0, eps, scale, size, dir.x as f32, dir.y as f32, dir.z as f32, 0.0]
        }
        None => [0.0; 8],
    }
}

fn section_clip(app: &SolveApp) -> [f32; 4] {
    match section_plane(app) {
        Some((o, n)) => [n.x as f32, n.y as f32, n.z as f32, n.dot(o) as f32],
        None => [0.0; 4],
    }
}

/// Space kept free at the top right of the viewport for the view cube (docked dialogs start below).
pub const VIEW_CUBE_CLEARANCE: f32 = 168.0;

/// Axis length as a multiple of the origin plane size (solid part 1, dashed beyond).
const AXIS_LEN: f64 = 1.8;

/// The hit as a plain selection (no command filter).
pub fn hit_sel(h: &Hit) -> Option<Sel> {
    Some(match h {
        Hit::SketchPoint { id, .. } => Sel::SketchPoint { id: id.clone() },
        Hit::SketchCurve { id, .. } => Sel::SketchCurve { id: id.clone() },
        Hit::Vertex { body, point } => Sel::Vertex { body: body.clone(), point: *point },
        Hit::Edge { body, index, mid } => Sel::Edge { body: body.clone(), index: *index, point: *mid },
        Hit::Profile { sketch, index } => Sel::Profile { sketch: *sketch, index: *index },
        Hit::Face { body, index, point } => Sel::Face { body: body.clone(), index: *index, point: *point },
        Hit::Plane { name, .. } => Sel::Plane { name: name.clone() },
        Hit::Axis { name } => Sel::Axis { name: name.clone() },
    })
}

/// What a click would pick: the first hit the open dialog's active input accepts, or with no
/// dialog the first hit (sketch entities only belong to the active sketch).
pub fn candidate(app: &SolveApp, hits: &[Hit]) -> Option<(Hit, Sel)> {
    if let Some(d) = app.dialog.as_ref().filter(|d| d.wants_picks()) {
        return hits.iter().find_map(|h| d.candidate(&app.session, h).map(|s| (h.clone(), s)));
    }
    let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
    hits.iter()
        .filter(|h| match h {
            Hit::SketchCurve { sketch, .. } => app.session.active_sketch == Some(*sketch) && !off("sketch") && !off("sketch_curves"),
            Hit::SketchPoint { .. } => !off("sketch") && !off("sketch_points"),
            Hit::Profile { .. } => !off("sketch"),
            Hit::Vertex { .. } => !off("vertices") && !app.ui.pick_bodies,
            Hit::Edge { .. } => !off("edges") && !app.ui.pick_bodies,
            Hit::Face { .. } => !off("faces") || app.ui.pick_bodies,
            Hit::Plane { .. } | Hit::Axis { .. } => true,
        })
        .find_map(|h| match h {
            Hit::Face { body, .. } if app.ui.pick_bodies => Some((h.clone(), Sel::Body { name: body.clone() })),
            _ => hit_sel(h).map(|s| (h.clone(), s)),
        })
}

/// The edge a selection refers to now: by index if it still passes through the stored point,
/// otherwise the edge nearest the point.
fn edge_of(m: &solvecraft_engine::geom::Mesh, index: usize, point: Vec3) -> Option<&Vec<Vec3>> {
    let tol = m.bounds().diagonal().max(1.0) * 1e-6;
    let near = |e: &Vec<Vec3>| e.windows(2).map(|w| point.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
    if let Some(e) = m.edges.get(index)
        && near(e) < tol
    {
        return Some(e);
    }
    m.edges.iter().min_by(|a, b| near(a).total_cmp(&near(b))).filter(|e| near(e) < tol * 1e3)
}

fn face_tris(sc: &mut GpuScene, m: &solvecraft_engine::geom::Mesh, face: usize, col: [u8; 4], lit: bool) {
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if *f as usize != face {
            continue;
        }
        for k in t {
            let i = *k as usize;
            if let (Some(p), Some(n)) = (m.positions.get(i), m.normals.get(i)) {
                sc.tri(p.to_f32(), if lit { n.to_f32() } else { [0.0; 3] }, col);
            }
        }
    }
}

fn edge_lines(sc: &mut GpuScene, e: &[Vec3], core: Color32, halo: Color32, w: f32) {
    for (col, width) in [(halo, w + 3.0), (core, w)] {
        for p in e.windows(2) {
            sc.line(p[0].to_f32(), p[1].to_f32(), c4(col), width, false);
        }
    }
}

fn profile_fill(sc: &mut GpuScene, app: &SolveApp, sketch: u64, index: usize, fill: Color32, edge: Color32) {
    let st = app.session.model.state();
    let Some(ss) = st.sketch(sketch) else { return };
    let Some(p) = ss.profiles.get(index) else { return };
    // Lift toward the eye so a profile on a face shows over it.
    let lift = app.cam.back() * (app.cam.half_height() * 0.002);
    let w = |q: solvecraft_engine::geom::Vec2| (ss.plane.to_world(q) + lift).to_f32();
    let col = c4(fill);
    // In the translucent pass (after the grid and edges) so it covers them.
    for [a, b, c] in p.region.triangulate(0.02) {
        sc.trans_tri(w(a), [0.0; 3], col);
        sc.trans_tri(w(b), [0.0; 3], col);
        sc.trans_tri(w(c), [0.0; 3], col);
    }
    for lp in std::iter::once(&p.region.outer).chain(&p.region.holes) {
        let poly = lp.polyline(0.02);
        let n = poly.len();
        for i in 0..n {
            if let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) {
                sc.line(w(*a), w(*b), c4(edge), 2.0, false);
            }
        }
    }
}

fn quad(sc: &mut GpuScene, q: &[Vec3; 4], fill: Color32, edge: Color32) {
    let col = c4(fill);
    for i in [0, 1, 2, 0, 2, 3] {
        if let Some(p) = q.get(i) {
            sc.trans_tri(p.to_f32(), [0.0; 3], col);
        }
    }
    for i in 0..4 {
        sc.line(q[i].to_f32(), q[(i + 1) % 4].to_f32(), c4(edge), 1.2, false);
    }
}

fn highlight_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.session.revision.hash(&mut h);
    serde_json::to_string(&app.highlighted()).unwrap_or_default().hash(&mut h);
    format!("{:?}", app.viewport.hover).hash(&mut h);
    app.viewport.hover_feature.hash(&mut h);
    app.viewport.hover_bodies.hash(&mut h);
    app.ui.pick_bodies.hash(&mut h);
    crate::theme::is_dark().hash(&mut h);
    app.preview.replaced.hash(&mut h);
    app.origin_visible().hash(&mut h);
    app.ui.hidden_origin.hash(&mut h);
    app.ui.hidden_bodies.hash(&mut h);
    app.cam.half_height().to_bits().hash(&mut h);
    app.cam.back().x.to_bits().hash(&mut h);
    h.finish()
}

/// The highlight scene: origin widget, construction planes, selection and hover.
fn build_highlight(app: &SolveApp) -> GpuScene {
    let t = Tokens::get();
    let mut sc = GpuScene::default();
    let st = app.session.world_state();
    let sel = app.highlighted();
    let hover = app.viewport.hover.as_ref();
    let size = origin_size(app.cam.half_height());
    // Origin planes and construction planes.
    let plane_state = |name: &str| {
        let selected = sel.iter().any(|x| matches!(x, Sel::Plane { name: n } if n == name));
        let hovered = matches!(hover, Some(Hit::Plane { name: n, .. }) if n == name);
        (selected, hovered)
    };
    let mut planes: Vec<(String, [Vec3; 4], Color32)> = origin_planes(app).into_iter().map(|(n, _, q)| (n.to_string(), q, t.origin_plane)).collect();
    planes.extend(construction_quads(app).into_iter().map(|(n, _, q)| (n, q, t.construction_plane)));
    for (name, q, base) in &planes {
        let (selected, hovered) = plane_state(name);
        let fill = if hovered {
            t.origin_plane_hover
        } else if selected {
            Color32::from_rgba_unmultiplied(t.sel_face.r(), t.sel_face.g(), t.sel_face.b(), 150)
        } else {
            *base
        };
        quad(&mut sc, q, fill, if selected { t.sel_edge } else { t.origin_plane_edge });
    }
    // Origin axes: solid near the origin, dashed beyond.
    for (name, dir) in origin_axes(app) {
        let col = match name {
            "X" => t.axis_x,
            "Y" => t.axis_y,
            _ => t.axis_z,
        };
        let on = sel.iter().any(|x| matches!(x, Sel::Axis { name: n } if n == name)) || matches!(hover, Some(Hit::Axis { name: n }) if n == name);
        let w = if on { 4.0 } else { 2.0 };
        sc.line([0.0; 3], (dir * size).to_f32(), c4(col), w, false);
        let dash = size * 0.08;
        let mut a = size + dash;
        while a < size * AXIS_LEN {
            sc.line((dir * a).to_f32(), (dir * (a + dash)).to_f32(), c4(col), w * 0.75, false);
            a += dash * 2.0;
        }
    }
    // Selected things.
    for x in &sel {
        match x {
            Sel::Face { body, index, .. } => {
                // A live preview stands in for the body: its old faces would fight with it.
                if let Some(b) = st.body(body).filter(|_| !app.preview.replaced.contains(body)) {
                    face_tris(&mut sc, &b.mesh(), *index, c4(t.sel_face), false);
                }
            }
            Sel::Body { name } => {
                if let Some(b) = st.body(name)
                    && !app.ui.hidden_bodies.contains(name)
                {
                    let m = b.mesh();
                    for f in 0..b.body.face_count() {
                        face_tris(&mut sc, &m, f, c4(t.body_selected), true);
                    }
                }
            }
            Sel::Edge { body, index, point } => {
                if let Some(b) = st.body(body)
                    && let Some(e) = edge_of(&b.mesh(), *index, *point)
                {
                    edge_lines(&mut sc, e, t.sel_edge, t.sel_edge_rim, 3.0);
                }
            }
            Sel::Profile { sketch, index } => profile_fill(&mut sc, app, *sketch, *index, t.sel_profile, t.sel_profile_edge),
            // A 3D sketch curve, in whichever sketch has it.
            Sel::SketchCurve { id } if st.sketches.iter().any(|ss| ss.sketch.wire_index(id).is_some()) => {
                for ss in st.sketches.iter() {
                    if let Some(w) = ss.sketch.wire_index(id).and_then(|i| ss.sketch.wires.get(i)) {
                        for q in w.pts.windows(2) {
                            sc.line(q[0].to_f32(), q[1].to_f32(), c4(t.sel_edge), 3.5, true);
                        }
                    }
                }
            }
            Sel::SketchCurve { id } => {
                if let Some(ss) = app.session.active_sketch.and_then(|sid| st.sketch(sid))
                    && let Some(ci) = ss.sketch.curve_index(id)
                {
                    for seg in ss.sketch.segs(ci) {
                        let pts = seg.polyline(0.02);
                        for w in pts.windows(2) {
                            sc.line(ss.plane.to_world(w[0]).to_f32(), ss.plane.to_world(w[1]).to_f32(), c4(t.sel_edge), 3.5, true);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Bodies made by the timeline item under the cursor, or whose browser row is.
    if app.viewport.hover_feature.is_some() || !app.viewport.hover_bodies.is_empty() {
        let k = t.hover_face_lift;
        let lit =
            |b: &solvecraft_engine::doc::ModelBody| Some(b.feature) == app.viewport.hover_feature || app.viewport.hover_bodies.contains(&b.name);
        for b in st.bodies.iter().filter(|b| lit(b) && !app.ui.hidden_bodies.contains(&b.name)) {
            let c = body_rgb(app, &b.name);
            let m = b.mesh();
            for f in 0..b.body.face_count() {
                face_tris(&mut sc, &m, f, [c.0.saturating_add(k), c.1.saturating_add(k), c.2.saturating_add(k), 255], true);
            }
            for (ei, e) in m.edges.iter().enumerate() {
                if !m.seams.get(ei).copied().unwrap_or(false) {
                    edge_lines(&mut sc, e, t.hover_edge, t.hover_edge_halo, 1.5);
                }
            }
        }
    }
    // Hover (pre-highlight).
    match hover {
        // Body selection: the whole body lights up.
        Some(Hit::Face { body, .. }) if app.ui.pick_bodies && app.dialog.is_none() && app.tool.is_none() => {
            if let Some(b) = st.body(body) {
                let k = t.hover_face_lift;
                let c = body_rgb(app, body);
                let m = b.mesh();
                for f in 0..b.body.face_count() {
                    face_tris(&mut sc, &m, f, [c.0.saturating_add(k), c.1.saturating_add(k), c.2.saturating_add(k), 255], true);
                }
            }
        }
        Some(Hit::Face { body, index, .. })
            if !app.preview.replaced.contains(body)
                && !sel.iter().any(|x| matches!(x, Sel::Face { body: b, index: i, .. } if b == body && i == index)) =>
        {
            if let Some(b) = st.body(body) {
                let k = t.hover_face_lift;
                let c = body_rgb(app, body);
                face_tris(&mut sc, &b.mesh(), *index, [c.0.saturating_add(k), c.1.saturating_add(k), c.2.saturating_add(k), 255], true);
                let m = b.mesh();
                for e in m.face_edges(u32::try_from(*index).unwrap_or(u32::MAX)) {
                    if let Some(p) = m.edges.get(e)
                        && !m.seams.get(e).copied().unwrap_or(false)
                    {
                        edge_lines(&mut sc, p, t.hover_edge, t.hover_edge_halo, 1.5);
                    }
                }
            }
        }
        Some(Hit::Edge { body, index, mid }) => {
            if let Some(b) = st.body(body)
                && let Some(e) = edge_of(&b.mesh(), *index, *mid)
            {
                let selected = sel.iter().any(|x| matches!(x, Sel::Edge { point, .. } if point.dist(*mid) < 1e-9));
                if !selected {
                    edge_lines(&mut sc, e, t.hover_edge, t.hover_edge_halo, 2.0);
                }
            }
        }
        Some(Hit::Profile { sketch, index })
            if !sel.iter().any(|x| matches!(x, Sel::Profile { sketch: s2, index: i2 } if s2 == sketch && i2 == index)) =>
        {
            profile_fill(&mut sc, app, *sketch, *index, t.hover_profile, t.hover_profile_edge);
        }
        _ => {}
    }
    sc
}

/// A body's display colour (appearance, material or imported colour), if it has one.
fn body_color(doc: &solvecraft_engine::doc::Document, b: &solvecraft_engine::doc::ModelBody) -> Option<Color32> {
    let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    doc.body_color(b).map(|[r, g, b]| Color32::from_rgb(to8(r), to8(g), to8(b)))
}

/// A body's display colour (else the theme's) as RGB.
fn body_rgb(app: &SolveApp, name: &str) -> (u8, u8, u8) {
    let st = app.session.model.state();
    let c = st.body(name).and_then(|b| body_color(&app.session.doc, b)).unwrap_or(Tokens::get().body);
    (c.r(), c.g(), c.b())
}

fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Point on the active sketch plane under the cursor (sketch coordinates), snapped to sketch
/// points within a few pixels and otherwise to a fine grid. Returns the snapped point and the
/// id of the sketch point it snapped to.
pub fn sketch_point_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec2, Option<String>)> {
    let sid = app.session.active_sketch?;
    let st = app.session.model.state();
    let ss = st.sketch(sid)?;
    for h in pick(app, proj, pos) {
        if let Hit::SketchPoint { id, at, .. } = h {
            return Some((at, Some(id)));
        }
    }
    let (o, d) = proj.ray(pos);
    let w = ss.plane.intersect_ray(o, d)?;
    let lp = ss.plane.to_local(w);
    let (minor, _) = grid_step(app.cam.half_height());
    let step = minor / 10.0;
    if let Some(p) = crate::sketch_tools::refine_snap(app, proj, &ss.sketch, &ss.plane, lp) {
        return Some(p);
    }
    // With Snap off (Sketch Palette) the point is where the cursor is.
    if ss.sketch.view.no_snap {
        return Some((lp, None));
    }
    Some((Vec2::new((lp.x / step).round() * step, (lp.y / step).round() * step), None))
}

pub fn delete_selection(app: &mut SolveApp) {
    crate::delete::delete_selection(app);
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    let resp = ui.interact(rect, ui.id().with("viewport"), Sense::click_and_drag());
    let (hover, scroll, mods, middle, secondary_down, primary_down, delta) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.smooth_scroll_delta.y,
            i.modifiers,
            i.pointer.middle_down(),
            i.pointer.secondary_down(),
            i.pointer.primary_down(),
            i.pointer.delta(),
        )
    });
    // The right button belongs to the marking menu while it is open.
    let secondary_down = secondary_down && app.viewport.context_menu.is_none();
    // The view cube handles its own clicks; the model underneath must not see them.
    let cube = Rect::from_center_size(pos2(rect.right() - 80.0, rect.top() + 80.0), vec2(150.0, 150.0));
    let inside = hover.is_some_and(|p| rect.contains(p) && !cube.contains(p));
    app.viewport.mouse = hover.filter(|_| inside);

    // ---- navigation ----
    let h = rect.height().max(1.0) as f64;
    if inside && scroll != 0.0 {
        let f = (-scroll as f64 * 0.0015 * crate::prefs::zoom_sign(app)).exp();
        let anchor = hover.map(|p| {
            let proj = projection(app, rect);
            // Toward the model's surface under the cursor, else the target's depth.
            if let Some(q) = pick_cached(app, &proj, p).iter().find_map(|h| if let Hit::Face { point, .. } = h { Some(*point) } else { None }) {
                return q;
            }
            let (o, d) = proj.ray(p);
            let n = app.cam.back();
            let den = d.dot(n);
            if den.abs() > 1e-9 { o + d * ((app.cam.target - o).dot(n) / den) } else { app.cam.target }
        });
        app.cancel_view_animation();
        app.cam.zoom_at(f, anchor);
    }
    let dragging = resp.dragged() || (inside && (middle || secondary_down));
    if dragging && delta != egui::Vec2::ZERO {
        let (dx, dy) = (delta.x as f64, delta.y as f64);
        let mode = if middle || secondary_down {
            crate::prefs::nav_mode(app, middle, secondary_down, mods)
        } else if primary_down {
            app.viewport.nav
        } else {
            None
        };
        if mode.is_some() {
            app.cancel_view_animation();
        }
        match mode {
            Some(NavMode::Orbit) => {
                let press = ui.input(|i| i.pointer.press_origin());
                crate::prefs::orbit(app, rect, dx, dy, press);
            }
            Some(NavMode::Pan) => app.cam.pan(dx, dy, h),
            Some(NavMode::Zoom) => app.cam.zoom_at((dy * 0.01).exp(), None),
            None => {}
        }
    }
    if !dragging {
        app.viewport.orbit_pivot = None;
    }
    if app.viewport.spin {
        // A click on the navigation bar (its own button) is not a stop.
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 22.0), vec2(320.0, 30.0));
        let (pressed, key, dt) = ui.input(|i| {
            (
                i.pointer.any_pressed() && inside && !i.pointer.press_origin().is_some_and(|p| bar.contains(p)),
                i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })),
                i.stable_dt,
            )
        });
        if pressed || key || dragging {
            app.viewport.spin = false;
        } else {
            app.cam.orbit(f64::from(dt.min(0.1)) * 30.0, 0.0);
            ui.ctx().request_repaint();
        }
    }
    if !app.cam.is_valid() {
        app.cam = Camera::default();
        app.fit_view();
    }

    let proj = projection(app, rect);
    let painter = ui.painter_at(rect);
    // Background gradient.
    let (top, bot) = (t.viewport_top, t.viewport_bottom);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bot);
    mesh.colored_vertex(rect.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));

    // ---- model ----
    let key = scene_key(app);
    let t0 = crate::now_ms();
    if let Some(_g) = app.viewport.gpu {
        if key != app.viewport.key {
            let sc = build_scene(app);
            if let Ok(mut slot) = app.viewport.slot.lock() {
                *slot = Some(sc);
            }
            app.viewport.key = key;
            app.viewport.build_ms = crate::now_ms() - t0;
        }
        let hk = highlight_key(app);
        if hk != app.viewport.hl_key {
            let hl = build_highlight(app);
            if let Ok(mut slot) = app.viewport.hl_slot.lock() {
                *slot = Some(hl);
            }
            app.viewport.hl_key = hk;
        }
        let ppp = ui.ctx().pixels_per_point();
        let cb = ViewportCallback {
            key: app.viewport.key,
            slot: app.viewport.slot.clone(),
            hl_key: app.viewport.hl_key,
            hl_slot: app.viewport.hl_slot.clone(),
            pv_key: app.preview.key,
            pv_slot: app.preview.slot.clone(),
            clip: section_clip(app),
            cap: t.section_cap.to_normalized_gamma_f32(),
            view_proj: proj.vp.to_f32(),
            back: proj.cam.back().to_f32(),
            size_px: [rect.width() * ppp, rect.height() * ppp],
            images: crate::ref_images::gpu_images(app),
            analysis: analysis_uniform(app),
            access: crate::access::map(app),
            outline: matches!(app.ui.visual_style, 0 | 3),
        };
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    } else {
        cpu_render(app, ui.ctx(), &painter, rect, &proj);
    }

    // ---- interaction ----
    // No picking while the view moves (orbit, pan, zoom): nothing can be hovered meanwhile.
    let navigating = scroll != 0.0 || ((middle || secondary_down) && delta != egui::Vec2::ZERO) || (app.viewport.nav.is_some() && primary_down);
    let hits = hover.filter(|_| inside && app.viewport.boxsel.is_none() && !navigating).map(|p| pick_cached(app, &proj, p)).unwrap_or_default();
    let cand = if app.tool.is_some() { hits.first().cloned().map(|h| (h, None)) } else { candidate(app, &hits).map(|(h, s)| (h, Some(s))) };
    app.viewport.hover = cand.as_ref().map(|c| c.0.clone());
    let add = mods.shift || mods.command || mods.ctrl;
    if let Some(p) = hover.filter(|_| inside) {
        if app.tool.is_some() {
            crate::tools::on_hover(app, &proj, p);
        }
        if resp.clicked() {
            if app.tool.is_some() {
                crate::tools::on_click(app, &proj, p);
            } else if app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
                if let Some(sel) = cand.and_then(|c| c.1)
                    && let Some(mut d) = app.dialog.take()
                {
                    d.pick(&app.session, sel);
                    app.dialog = Some(d);
                }
            } else {
                // Dimensions and constraint glyphs sit on top of the geometry.
                let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
                let sketching = app.session.active_sketch.is_some();
                // A dimension's text wins; its lines only where no sketch point or curve is (a
                // diameter's line runs through the centre point).
                let on_geometry = matches!(cand.as_ref().map(|c| &c.0), Some(Hit::SketchPoint { .. } | Hit::SketchCurve { .. }));
                let on_dim = crate::dim_view::hit_text(p)
                    .or_else(|| if on_geometry { None } else { crate::dim_view::hit(p) })
                    .filter(|_| sketching && !off("dimensions"));
                let on_glyph = crate::sketch_tools::glyph_at(p).filter(|_| sketching && !off("constraints"));
                if !crate::sketch3d::click(app, &proj, p) {
                    match on_dim.or(on_glyph) {
                        Some(id) => select(app, Some(Sel::SketchConstraint { id }), add),
                        None => select(app, cand.and_then(|c| c.1), add),
                    }
                }
            }
        }
        // Double-click a dimension: edit its value in place.
        let dim_edit = resp.double_clicked() && app.tool.is_none() && app.dialog.is_none() && crate::dim_view::double_click(app, p);
        // Double-click a sketch curve: select the chain joined to it end to end.
        if resp.double_clicked()
            && !dim_edit
            && app.tool.is_none()
            && app.dialog.is_none()
            && let Some(Hit::SketchCurve { sketch, id, .. }) = app.viewport.hover.clone()
            && app.session.active_sketch == Some(sketch)
            && let Ok(v) = app.session.execute("sketch.chain", &json!({ "curve": id }))
        {
            let items: Vec<Sel> =
                v["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(|id| Sel::SketchCurve { id: id.into() }).collect();
            let _ = app.run("select.set", json!({ "items": items, "add": add }));
        }
        // Double-click an edge: select the edges that continue it smoothly (the loop).
        if resp.double_clicked()
            && !dim_edit
            && app.tool.is_none()
            && app.dialog.is_none()
            && let Some(Hit::Edge { body, index, .. }) = app.viewport.hover.clone()
            && let Some(b) = app.session.model.state().body(&body).cloned()
        {
            let m = b.mesh();
            let items: Vec<Sel> = m
                .tangent_chain(index, 2f64.to_radians())
                .into_iter()
                .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                .collect();
            let _ = app.run("select.set", json!({ "items": items, "add": add }));
        }
        if resp.secondary_clicked() && delta == egui::Vec2::ZERO {
            if app.tool.is_some() {
                crate::tools::finish(app);
            } else if app.dialog.is_none() {
                crate::context_menu::open(app, p);
            }
        }
    }
    // Dragging a sketch point of the active sketch moves it; the solver keeps the constraints.
    if app.tool.is_none() && app.dialog.is_none() && app.viewport.nav.is_none() && app.session.active_sketch.is_some() {
        let on_plane = |app: &SolveApp, p: Pos2| {
            let ss = app.session.active_sketch.and_then(|sid| app.session.model.state().sketch(sid).cloned())?;
            let (o, d) = proj.ray(p);
            ss.plane.intersect_ray(o, d).map(|w| ss.plane.to_local(w))
        };
        // The 3D triad, then a dimension's text, follow a drag that starts on them.
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(a) = ui.input(|i| i.pointer.press_origin())
            && !crate::sketch3d::drag_start(app, &proj, a)
            && let Some(q) = on_plane(app, a)
        {
            crate::dim_view::drag_start(app, a, q);
        }
        if crate::sketch3d::dragging()
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(p) = hover
        {
            crate::sketch3d::drag_to(app, &proj, p);
        }
        if crate::dim_view::dragging()
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(q) = hover.and_then(|p| on_plane(app, p))
        {
            crate::dim_view::drag_to(app, q);
        }
        // A point or a curve of the sketch being edited follows a drag that starts on it.
        if resp.drag_started_by(egui::PointerButton::Primary)
            && !crate::dim_view::dragging()
            && !crate::sketch3d::dragging()
            && let Some(a) = ui.input(|i| i.pointer.press_origin())
        {
            let active = app.session.active_sketch;
            match pick(app, &proj, a).into_iter().next() {
                Some(Hit::SketchPoint { id, .. }) => app.viewport.point_drag = Some((id, app.session.undo.len(), None)),
                Some(Hit::SketchCurve { sketch, id, .. }) if Some(sketch) == active => {
                    app.viewport.point_drag = on_plane(app, a).map(|q| (id, app.session.undo.len(), Some(q)));
                }
                _ => {}
            }
        }
        if let Some((id, depth, grab)) = app.viewport.point_drag.clone()
            && resp.dragged_by(egui::PointerButton::Primary)
            && let Some(q) = hover.and_then(|p| on_plane(app, p))
        {
            let from = grab.unwrap_or(q);
            if app.session.execute("sketch.drag", &json!({"entity": id, "from": [from.x, from.y], "to": [q.x, q.y]})).is_ok() {
                app.session.undo.truncate(depth + 1);
            }
            if grab.is_some() {
                app.viewport.point_drag = Some((id, depth, Some(q)));
            }
        }
        if resp.drag_stopped() {
            app.viewport.point_drag = None;
            crate::dim_view::drag_end();
            crate::sketch3d::drag_end();
        }
    }
    // Box selection: a primary drag on the model when no navigation mode is on.
    if app.tool.is_none()
        && app.viewport.nav.is_none()
        && app.viewport.point_drag.is_none()
        && !crate::dim_view::dragging()
        && !crate::sketch3d::dragging()
    {
        let origin = ui.input(|i| i.pointer.press_origin());
        if resp.dragged_by(egui::PointerButton::Primary)
            && let (Some(a), Some(b)) = (origin, hover)
            && rect.contains(a)
            && !cube.contains(a)
        {
            app.viewport.boxsel = Some(BoxSel::from_drag(a, b));
        }
        if resp.drag_stopped()
            && let Some(bx) = app.viewport.boxsel.take()
        {
            box_select(app, &proj, bx, add);
        }
    } else {
        app.viewport.boxsel = None;
    }
    crate::ref_images::show(app, ui, &painter, &proj);
    overlays(app, &painter, &proj);
    crate::dim_view::show(app, ui, &painter, &proj);
    crate::sketch3d::show(app, &painter, &proj);
    crate::inference::show(app, &painter, &proj);
    hover_highlight(app, &painter, &proj);
    points_2d(app, &painter, &proj);
    if let Some(bx) = app.viewport.boxsel {
        let col = if bx.crossing { t.box_crossing } else { t.box_window };
        painter.rect_filled(bx.rect, 0.0, col.gamma_multiply(0.12));
        if bx.crossing {
            let c = [bx.rect.left_top(), bx.rect.right_top(), bx.rect.right_bottom(), bx.rect.left_bottom()];
            for i in 0..4 {
                painter.add(Shape::dashed_line(&[c[i], c[(i + 1) % 4]], Stroke::new(1.2, col), 6.0, 4.0));
            }
        } else {
            painter.rect_stroke(bx.rect, 0.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
        }
    }
    if let Some(tl) = app.tool.as_ref() {
        crate::tools::preview(app, tl, &painter, &proj);
    }
    crate::canvas::show(app, ui, &painter, &proj);
    crate::sketch_tools::show(app, ui, &painter, &proj);
    crate::sketch_dims::show(app, ui, &proj);
    view_cube(app, ui, rect);
    nav_bar(app, ui, rect);
    // Status chip.
    if let Some((msg, at, err)) = app.status.clone() {
        if crate::now_ms() - at < 6000.0 {
            let pos = pos2(rect.left() + 12.0, rect.bottom() - 46.0);
            let galley = painter.layout_no_wrap(msg, FontId::proportional(12.5), if err { t.error } else { t.text });
            let r = Rect::from_min_size(pos, galley.size() + vec2(16.0, 8.0));
            painter.rect_filled(r, 4.0, t.overlay);
            painter.galley(pos + vec2(8.0, 4.0), galley, t.text);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        } else {
            app.status = None;
        }
    }
}

/// A click without a command: select the candidate (replacing the selection, or toggling it
/// in with Shift/Ctrl); a click on nothing clears.
fn select(app: &mut SolveApp, sel: Option<Sel>, add: bool) {
    match sel {
        Some(x) => {
            let item = serde_json::to_value(&x).unwrap_or_default();
            if add && app.session.selection.contains(&x) {
                let rest: Vec<Sel> = app.session.selection.iter().filter(|y| **y != x).cloned().collect();
                let _ = app.run("select.set", json!({ "items": rest }));
            } else {
                let _ = app.run("select.set", json!({"items": [item], "add": add}));
            }
        }
        None if !add => {
            let _ = app.run("select.clear", json!({}));
        }
        None => {}
    }
}

/// What a box selects: sketch entities while sketching, otherwise what the open dialog's
/// active input takes (edges, faces or bodies), otherwise bodies.
fn box_select(app: &mut SolveApp, proj: &Proj, bx: BoxSel, add: bool) {
    use crate::selection::{BODIES, EDGES, FACES, PROFILES};
    let st = app.session.world_state();
    let mut out: Vec<Sel> = Vec::new();
    let to2 = |pts: &[Vec3]| -> Vec<Pos2> { pts.iter().filter_map(|p| proj.to_screen(*p)).collect() };
    if app.dialog.is_none()
        && let Some(ss) = app.session.active_sketch.and_then(|sid| st.sketch(sid))
    {
        // What the selection filter lets through (window: wholly inside; crossing: touching).
        let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
        for (i, c) in ss.sketch.curves.iter().enumerate().filter(|_| !off("sketch") && !off("sketch_curves")) {
            if c.link.is_some() && ss.sketch.view.hide_projected {
                continue;
            }
            let pts: Vec<Vec3> = ss.sketch.segs(i).iter().flat_map(|sg| sg.polyline(0.05)).map(|q| ss.plane.to_world(q)).collect();
            if bx.takes(&to2(&pts)) {
                out.push(Sel::SketchCurve { id: c.id.clone() });
            }
        }
        for (i, p) in ss.sketch.points.iter().enumerate().filter(|_| !off("sketch") && !off("sketch_points")) {
            if i != 0 && solvecraft_engine::view::sketch_point_visible(&ss.sketch, i) && bx.takes(&to2(&[ss.plane.to_world(p.pos)])) {
                out.push(Sel::SketchPoint { id: p.id.clone() });
            }
        }
        if !off("dimensions") {
            for (id, r) in crate::dim_view::text_rects() {
                if bx.takes(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]) {
                    out.push(Sel::SketchConstraint { id });
                }
            }
        }
        if !off("constraints") {
            for (id, c) in crate::sketch_tools::glyph_positions() {
                if bx.takes(&[c]) {
                    out.push(Sel::SketchConstraint { id });
                }
            }
        }
    } else {
        let accept = app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept).unwrap_or(BODIES);
        for b in &st.bodies {
            if app.ui.hidden_bodies.contains(&b.name) {
                continue;
            }
            let m = b.mesh();
            if accept & EDGES != 0 {
                for (ei, e) in m.edges.iter().enumerate() {
                    if !m.seams.get(ei).copied().unwrap_or(false) && bx.takes(&to2(e)) {
                        out.push(Sel::Edge { body: b.name.clone(), index: ei, point: polyline_mid(e) });
                    }
                }
            } else if accept & FACES != 0 {
                for f in 0..b.body.face_count() {
                    let mut pts: Vec<Vec3> = Vec::new();
                    for e in m.face_edges(u32::try_from(f).unwrap_or(u32::MAX)) {
                        pts.extend(m.edges.get(e).into_iter().flatten().copied());
                    }
                    let point = m
                        .triangles
                        .iter()
                        .zip(&m.tri_face)
                        .find(|(_, tf)| **tf as usize == f)
                        .and_then(|(t, _)| m.tri(t))
                        .map(|[a, bb, c]| (a + bb + c) / 3.0);
                    if let Some(point) = point
                        && bx.takes(&to2(&pts))
                    {
                        out.push(Sel::Face { body: b.name.clone(), index: f, point });
                    }
                }
            } else if accept & BODIES != 0 {
                let crossing_hit = m.edges.iter().any(|e| bx.takes(&to2(e)));
                let all_in = m.edges.iter().all(|e| bx.takes(&to2(e)));
                if (bx.crossing && crossing_hit) || (!bx.crossing && all_in && !m.edges.is_empty()) {
                    out.push(Sel::Body { name: b.name.clone() });
                }
            } else if accept & PROFILES != 0 {
                for sid in visible_sketches(app) {
                    let Some(ss) = st.sketch(sid) else { continue };
                    for (pi, p) in ss.profiles.iter().enumerate() {
                        let pts: Vec<Vec3> = p.region.outer.polyline(0.05).into_iter().map(|q| ss.plane.to_world(q)).collect();
                        if bx.takes(&to2(&pts)) {
                            out.push(Sel::Profile { sketch: sid, index: pi });
                        }
                    }
                }
                break;
            }
        }
    }
    if let Some(mut d) = app.dialog.take() {
        d.take_box(out, add);
        app.dialog = Some(d);
        return;
    }
    let items = serde_json::to_value(&out).unwrap_or_default();
    let _ = app.run("select.set", json!({"items": items, "add": add}));
}

/// Screen-space dots: the origin point, and hovered or selected body vertices.
fn points_2d(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    if app.origin_visible()
        && !app.ui.hidden_origin.iter().any(|h| h == "O")
        && let Some(o) = proj.to_screen(Vec3::ZERO)
    {
        painter.circle(o, 4.5, t.origin_point, Stroke::new(1.0, Color32::from_gray(110)));
    }
    for x in app.highlighted() {
        if let Sel::Vertex { point, .. } = x
            && let Some(p) = proj.to_screen(point)
        {
            painter.circle(p, 4.0, t.sel_vertex, Stroke::new(1.5, t.sel_edge));
        }
    }
    if let Some(Hit::Vertex { point, .. }) = &app.viewport.hover
        && let Some(p) = proj.to_screen(*point)
    {
        painter.circle(p, 4.5, t.hover_edge_halo, Stroke::new(1.5, t.hover_edge));
    }
}

/// Hover feedback for the active sketch's points and curves (drawn on top of everything).
fn hover_highlight(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let col = Tokens::get().hover_profile_edge;
    let st = app.session.model.state();
    match &app.viewport.hover {
        Some(Hit::SketchCurve { sketch, id, .. }) => {
            if let Some(ss) = st.sketch(*sketch)
                && let Some(ci) = ss.sketch.curve_index(id)
            {
                for seg in ss.sketch.segs(ci) {
                    let pts: Vec<Pos2> = seg.polyline(0.05).iter().filter_map(|q| proj.to_screen(ss.plane.to_world(*q))).collect();
                    painter.add(Shape::line(pts, Stroke::new(3.0, col)));
                }
            }
        }
        Some(Hit::SketchPoint { sketch, at, .. }) => {
            if let Some(ss) = st.sketch(*sketch)
                && let Some(p) = proj.to_screen(ss.plane.to_world(*at))
            {
                painter.circle_stroke(p, 6.0, Stroke::new(2.0, col));
            }
        }
        _ => {}
    }
}

/// Sketch points, dimension labels and constraint glyphs of the active sketch.
fn overlays(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    let st = app.session.model.state();
    let Some(sid) = app.session.active_sketch else { return };
    let Some(ss) = st.sketch(sid) else { return };
    let sk = &ss.sketch;
    let locked = sk.locked_points();
    for (i, p) in sk.points.iter().enumerate() {
        if !solvecraft_engine::view::sketch_point_visible(sk, i) {
            continue;
        }
        let hidden = (sk.view.hide_points && i != 0) || (sk.view.hide_projected && p.link.is_some());
        let Some(sp) = proj.to_screen(ss.plane.to_world(p.pos)).filter(|_| !hidden) else { continue };
        let det = ss.report.point_determined.get(i).copied().unwrap_or(false);
        let c = if p.link.is_none() && locked.get(i).copied().unwrap_or(false) {
            t.sketch_locked
        } else if det {
            t.sketch_point
        } else {
            t.sketch_point_free
        };
        if i == 0 {
            painter.circle(sp, 3.5, Color32::from_rgb(240, 200, 60), Stroke::new(1.0, t.body_edge));
        } else {
            painter.rect_filled(Rect::from_center_size(sp, vec2(5.0, 5.0)), 0.0, c);
        }
    }
    for c in sk.constraints.iter().filter(|c| !c.kind.is_dimension() && !sk.view.hide_constraints) {
        glyph(painter, proj, ss, &c.kind);
    }
    // Sketch status.
    if let Some(r) = app.viewport.rect {
        let msg = if !ss.report.ok() {
            format!("{} — constraints conflict", ss.name)
        } else if ss.report.dof == 0 {
            format!("{} — fully constrained", ss.name)
        } else {
            format!("{} — {} degrees of freedom", ss.name, ss.report.dof)
        };
        let c = if ss.report.ok() { t.sketch_accent } else { t.error };
        painter.text(pos2(r.left() + 12.0, r.top() + 12.0), Align2::LEFT_TOP, msg, FontId::proportional(13.0), c);
    }
}

fn line_mid(sk: &solvecraft_engine::sketch::Sketch, l: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(l)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

/// Small constraint markers next to the constrained geometry.
fn glyph(painter: &egui::Painter, proj: &Proj, ss: &solvecraft_engine::doc::SolvedSketch, k: &ConstraintKind) {
    use ConstraintKind::*;
    let sk = &ss.sketch;
    let (anchor, label) = match *k {
        Horizontal { l } => (line_mid(sk, l).map(|(a, b)| (a + b) * 0.5), "H"),
        Vertical { l } => (line_mid(sk, l).map(|(a, b)| (a + b) * 0.5), "V"),
        Parallel { a, .. } => (line_mid(sk, a).map(|(p, q)| (p + q) * 0.5), "//"),
        Perpendicular { a, .. } => (line_mid(sk, a).map(|(p, q)| (p + q) * 0.5), "L"),
        Tangent { a, .. } => (line_mid(sk, a).map(|(p, q)| (p + q) * 0.5).or_else(|| sk.center(a)), "T"),
        Equal { a, .. } => (line_mid(sk, a).map(|(p, q)| (p + q) * 0.5).or_else(|| sk.center(a)), "="),
        Fix { p } => (sk.point(p), "F"),
        _ => (None, ""),
    };
    let Some(a) = anchor.and_then(|a| proj.to_screen(ss.plane.to_world(a))) else { return };
    let r = Rect::from_center_size(a + vec2(10.0, -10.0), vec2(13.0, 13.0));
    let t = Tokens::get();
    painter.rect_filled(r, 2.0, t.glyph_bg);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, t.glyph_edge), egui::StrokeKind::Inside);
    painter.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(10.0), t.glyph_text);
}

fn cpu_render(app: &mut SolveApp, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, proj: &Proj) {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    scene_key(app).hash(&mut h);
    for v in [proj.cam.yaw, proj.cam.pitch, proj.cam.distance, proj.cam.target.x, proj.cam.target.y, proj.cam.target.z] {
        v.to_bits().hash(&mut h);
    }
    (rect.width() as u32, rect.height() as u32).hash(&mut h);
    let key = h.finish();
    if app.viewport.cpu.as_ref().is_none_or(|(k, _)| *k != key) {
        let scene = solvecraft_engine::view::scene(&app.session, &proj.cam);
        let w = (rect.width() as usize).clamp(16, 4096);
        let hh = (rect.height() as usize).clamp(16, 4096);
        let c = solvecraft_engine::render::render(&scene, &proj.cam, w, hh);
        let img = egui::ColorImage::from_rgba_unmultiplied([c.w, c.h], &c.rgba);
        let tex = ctx.load_texture("sc_cpu_viewport", img, egui::TextureOptions::LINEAR);
        app.viewport.cpu = Some((key, tex));
    }
    if let Some((_, tex)) = &app.viewport.cpu {
        painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// The view cube (top right): click a face for a standard view; the house resets.
fn view_cube(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let t = Tokens::get();
    let c = pos2(rect.right() - 80.0, rect.top() + 80.0);
    let s = 32.0f64;
    let (r, u, b) = app.cam.basis();
    let to2 = |p: Vec3| -> (Pos2, f64) { (pos2(c.x + (p.dot(r) * s) as f32, c.y - (p.dot(u) * s) as f32), p.dot(b)) };
    let faces: [(Vec3, &str, StandardView); 6] = [
        (Vec3::new(0.0, -1.0, 0.0), "FRONT", StandardView::Front),
        (Vec3::new(0.0, 1.0, 0.0), "BACK", StandardView::Back),
        (Vec3::new(1.0, 0.0, 0.0), "RIGHT", StandardView::Right),
        (Vec3::new(-1.0, 0.0, 0.0), "LEFT", StandardView::Left),
        (Vec3::Z, "TOP", StandardView::Top),
        (Vec3::new(0.0, 0.0, -1.0), "BOTTOM", StandardView::Bottom),
    ];
    let painter = ui.painter_at(rect);
    // Clicked view direction (target → eye): a face, or an edge/corner zone of a face.
    let mut clicked: Option<Vec3> = None;
    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut order: Vec<usize> = (0..6).collect();
    order.sort_by(|a, b2| faces[*a].0.dot(b).total_cmp(&faces[*b2].0.dot(b)));
    for i in order {
        let (n, label, _) = faces[i];
        if n.dot(b) <= 1e-3 {
            continue;
        }
        let (e1, e2) = if n.z.abs() > 0.5 {
            (Vec3::X, Vec3::Y)
        } else if n.x.abs() > 0.5 {
            (Vec3::Y, Vec3::Z)
        } else {
            (Vec3::X, Vec3::Z)
        };
        let corners = [n - e1 - e2, n + e1 - e2, n + e1 + e2, n - e1 + e2];
        let pts: Vec<Pos2> = corners.iter().map(|p| to2(*p).0).collect();
        let poly_hover = hover.is_some_and(|h| point_in_poly(h, &pts));
        let shade = (0.75 + 0.25 * n.dot(b)) as f32;
        painter.add(Shape::convex_polygon(pts.clone(), t.cube_face.gamma_multiply(shade).to_opaque(), Stroke::new(1.0, t.cube_edge)));
        // Hover zone: the outer band of a face picks the edge or corner it borders.
        let zone = hover.filter(|_| poly_hover).and_then(|h| cube_zone(&pts, h));
        if let Some((zi, zj)) = zone {
            let lo = |k: i32| match k {
                -1 => (-1.0, -1.0 + 2.0 * CUBE_BAND),
                0 => (-1.0 + 2.0 * CUBE_BAND, 1.0 - 2.0 * CUBE_BAND),
                _ => (1.0 - 2.0 * CUBE_BAND, 1.0),
            };
            let ((a0, a1), (b0, b1)) = (lo(zi), lo(zj));
            let zp: Vec<Pos2> = [(a0, b0), (a1, b0), (a1, b1), (a0, b1)].iter().map(|(x, y)| to2(n + e1 * *x + e2 * *y).0).collect();
            painter.add(Shape::convex_polygon(zp, t.accent_soft, Stroke::NONE));
        }
        let center = to2(n).0;
        if n.dot(b) > 0.35 {
            painter.text(center, Align2::CENTER_CENTER, label, FontId::proportional(9.5), Color32::from_rgb(40, 44, 52));
        }
        if let Some((zi, zj)) = zone
            && ui.input(|i| i.pointer.primary_clicked())
        {
            clicked = Some(n + e1 * zi as f64 + e2 * zj as f64);
        }
    }
    // Axis triad at the cube's corner.
    let o = Vec3::new(-1.0, -1.0, -1.0);
    for (d, col) in [(Vec3::X, colors::AXIS_X), (Vec3::Y, colors::AXIS_Y), (Vec3::Z, colors::AXIS_Z)] {
        let (a, _) = to2(o);
        let (bb, _) = to2(o + d * 2.6);
        painter.line_segment([a, bb], Stroke::new(2.0, Color32::from_rgb(col.0, col.1, col.2)));
    }
    let home = Rect::from_center_size(pos2(c.x - 52.0, c.y - 48.0), vec2(18.0, 18.0));
    let hr = ui.interact(home, ui.id().with("vc_home"), Sense::click());
    icons::paint(&painter, home, "home", t.icon, if hr.hovered() { t.accent_soft } else { t.overlay }, t.accent);
    if hr.on_hover_text("Home view").clicked() {
        app.animate_view("home");
    }
    if let Some(dir) = clicked {
        let to = app.cam.looking_from(dir);
        app.animate_to(to);
    }
}

/// Width of the edge/corner band of a view cube face, as a fraction of the face.
const CUBE_BAND: f64 = 0.2;

/// Which of the 3×3 zones of a projected cube face (corners in order −−, +−, ++, −+) the point
/// is over: (−1|0|1, −1|0|1).
pub(crate) fn cube_zone(pts: &[Pos2], p: Pos2) -> Option<(i32, i32)> {
    let [c0, c1, _, c3] = pts else { return None };
    let (ux, uy) = ((c1.x - c0.x) as f64, (c1.y - c0.y) as f64);
    let (vx, vy) = ((c3.x - c0.x) as f64, (c3.y - c0.y) as f64);
    let det = ux * vy - uy * vx;
    if det.abs() < 1e-9 {
        return None;
    }
    let (px, py) = ((p.x - c0.x) as f64, (p.y - c0.y) as f64);
    let a = (px * vy - py * vx) / det;
    let b = (ux * py - uy * px) / det;
    if !(-1e-6..=1.0 + 1e-6).contains(&a) || !(-1e-6..=1.0 + 1e-6).contains(&b) {
        return None;
    }
    let k = |t: f64| {
        if t < CUBE_BAND {
            -1
        } else if t > 1.0 - CUBE_BAND {
            1
        } else {
            0
        }
    };
    Some((k(a), k(b)))
}

fn point_in_poly(p: Pos2, poly: &[Pos2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Navigation bar (bottom centre).
/// Turn the view to face the selected planar face or plane, else the active sketch.
pub fn look_at_selection(app: &mut SolveApp) {
    let target = app.session.selection.iter().find_map(|s| match s {
        Sel::Face { body, index, point } => crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| (*point, n)),
        Sel::Plane { name } => solvecraft_engine::geom::Plane::named(name).map(|p| (p.origin, p.normal())),
        _ => None,
    });
    match target {
        Some((p, n)) => {
            let mut to = app.cam.looking_from(n);
            to.target = p;
            app.animate_to(to);
        }
        None => crate::dialogs::look_at_sketch(app),
    }
}

fn nav_bar(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let t = Tokens::get();
    let items: [(&str, &str); 10] = [
        ("orbit", "Orbit (drag; or Shift+middle / right drag)"),
        ("spin", "Continuous orbit (stops at the next click or key)"),
        ("pan", "Pan (drag; or middle drag)"),
        ("zoom", "Zoom (drag; or wheel)"),
        ("fit", "Fit (F6)"),
        ("lookat", "Look At the selected face or plane (or the active sketch)"),
        ("home", "Home view"),
        ("perspective", "Perspective / orthographic"),
        ("style", "Visual style: shaded with edges, shaded, wireframe, shaded with hidden edges"),
        ("settings", "Grid on/off"),
    ];
    let w = items.len() as f32 * 30.0 + 10.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 22.0), vec2(w, 30.0));
    let painter = ui.painter_at(rect);
    painter.rect(bar, 6.0, t.overlay, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, (icon, tip)) in items.iter().enumerate() {
        let br = Rect::from_min_size(pos2(bar.left() + 5.0 + i as f32 * 30.0, bar.top() + 2.0), vec2(26.0, 26.0));
        let resp = ui.interact(br, ui.id().with(("nav", *icon)), Sense::click());
        let active = match *icon {
            "orbit" => app.viewport.nav == Some(NavMode::Orbit),
            "pan" => app.viewport.nav == Some(NavMode::Pan),
            "zoom" => app.viewport.nav == Some(NavMode::Zoom),
            "perspective" => app.ui.perspective,
            "settings" => app.ui.show_grid,
            "spin" => app.viewport.spin,
            _ => false,
        };
        if active || resp.hovered() {
            painter.rect_filled(br, 4.0, if active { t.accent_soft } else { t.hover });
        }
        icons::paint(&painter, br.shrink(4.0), icon, t.icon, t.icon_fill, t.accent);
        if resp.on_hover_text(*tip).clicked() {
            let toggle = |m: NavMode, cur: Option<NavMode>| if cur == Some(m) { None } else { Some(m) };
            match *icon {
                "orbit" => app.viewport.nav = toggle(NavMode::Orbit, app.viewport.nav),
                "pan" => app.viewport.nav = toggle(NavMode::Pan, app.viewport.nav),
                "zoom" => app.viewport.nav = toggle(NavMode::Zoom, app.viewport.nav),
                "fit" => app.animate_view("fit"),
                "spin" => {
                    app.cancel_view_animation();
                    app.viewport.spin = !app.viewport.spin;
                }
                "home" => app.animate_view("home"),
                "lookat" => look_at_selection(app),
                "style" => app.ui.visual_style = (app.ui.visual_style + 1) % 4,
                "perspective" => app.ui.perspective = !app.ui.perspective,
                _ => app.ui.show_grid = !app.ui.show_grid,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_cube_zones() {
        // A square face 100 px wide, corners in order −−, +−, ++, −+ (screen y down).
        let pts = [pos2(0.0, 100.0), pos2(100.0, 100.0), pos2(100.0, 0.0), pos2(0.0, 0.0)];
        assert_eq!(cube_zone(&pts, pos2(50.0, 50.0)), Some((0, 0)));
        assert_eq!(cube_zone(&pts, pos2(5.0, 95.0)), Some((-1, -1)));
        assert_eq!(cube_zone(&pts, pos2(95.0, 50.0)), Some((1, 0)));
        assert_eq!(cube_zone(&pts, pos2(50.0, 5.0)), Some((0, 1)));
        assert_eq!(cube_zone(&pts, pos2(150.0, 50.0)), None);
    }
}
