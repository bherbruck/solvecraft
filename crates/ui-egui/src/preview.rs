//! Live operation previews. While a feature dialog is open, its commands are evaluated on a
//! scratch copy of the session ([`solvecraft_engine::Session::preview`]), off the UI thread
//! where threads exist, and the result is drawn in place of the bodies it changes: new surface
//! tinted, removed material as a translucent ghost. The last good preview stays up while the
//! next one computes or when an input makes the feature fail (the error shows in the dialog).
//! Nothing here touches the document or the undo history.

use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::Value;
use solvecraft_engine::Session;
use solvecraft_engine::doc::ModelState;
use solvecraft_engine::geom::{Mesh, Vec3};

use crate::SolveApp;
use crate::gpu::{GpuScene, SceneSlot};
use crate::theme::Tokens;

/// Colours a preview is drawn with (straight sRGBA).
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub body: [u8; 4],
    pub added: [u8; 4],
    pub removed: [u8; 4],
    pub edge: [u8; 4],
}

impl Colors {
    pub fn from_tokens(t: &Tokens) -> Colors {
        let c = |c: egui::Color32| c.to_srgba_unmultiplied();
        Colors { body: c(t.body), added: c(t.preview_add), removed: c(t.preview_cut), edge: c(t.body_edge) }
    }
}

/// A computed preview: the scene to draw and the bodies it stands in for.
pub struct Built {
    pub scene: GpuScene,
    pub replaced: Vec<String>,
    /// Why the feature failed, when the scene only shows its tool body.
    pub error: Option<String>,
}

type Job = Result<Built, String>;

#[derive(Default)]
pub struct PreviewState {
    /// Key of the commands whose result (or error) is shown.
    shown_key: Option<u64>,
    running: Option<(u64, Receiver<Job>)>,
    /// Bodies the preview stands in for (hidden in the model scene).
    pub replaced: Vec<String>,
    /// The preview scene for the GPU and its key (changes whenever the scene does).
    pub slot: SceneSlot,
    pub key: u64,
    /// Is a preview scene up?
    pub active: bool,
    /// Why the current inputs don't make a feature (the last good preview stays up).
    pub error: Option<String>,
    /// A preview is being computed.
    pub busy: bool,
    /// Time the last preview took (ms), for `ui.inspect`.
    pub ms: f64,
    started: f64,
}

impl PreviewState {
    fn clear(&mut self) {
        self.running = None;
        self.shown_key = None;
        self.error = None;
        self.busy = false;
        if self.active || !self.replaced.is_empty() {
            self.active = false;
            self.replaced.clear();
            self.set_scene(GpuScene::default());
        }
    }

    fn set_scene(&mut self, sc: GpuScene) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(sc);
        }
        self.key = self.key.wrapping_add(1);
    }

    fn apply(&mut self, key: u64, job: Job) {
        self.shown_key = Some(key);
        self.ms = crate::now_ms() - self.started;
        match job {
            Ok(b) => {
                self.replaced = b.replaced;
                self.active = true;
                self.error = b.error;
                self.set_scene(b.scene);
            }
            Err(e) => self.error = Some(e),
        }
    }
}

/// Keep the preview in step with the open dialog: start a computation when its inputs changed,
/// take finished results. Called once per frame.
pub fn update(app: &mut SolveApp, ctx: &egui::Context) {
    let cmds = match app.dialog.as_ref().filter(|d| d.previews()).map(|d| crate::dialogs::apply_commands(app, d)) {
        Some(Ok(c)) if !c.is_empty() => c,
        // No dialog, or inputs still missing: nothing to preview.
        _ => {
            app.preview.clear();
            return;
        }
    };
    let colors = Colors::from_tokens(&Tokens::get());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&cmds).unwrap_or_default().hash(&mut h);
    app.session.revision.hash(&mut h);
    format!("{colors:?}").hash(&mut h);
    let key = h.finish();
    let pv = &mut app.preview;
    if let Some((k, rx)) = &pv.running {
        let k = *k;
        match rx.try_recv() {
            Ok(job) => {
                pv.running = None;
                pv.apply(k, job);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                pv.running = None;
                pv.apply(k, Err("the preview failed".into()));
            }
        }
    }
    if pv.running.is_none() && pv.shown_key != Some(key) {
        pv.started = crate::now_ms();
        let scratch = app.session.scratch();
        spawn(&mut app.preview, key, scratch, cmds, colors);
    }
    app.preview.busy = app.preview.running.is_some();
    if app.preview.busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(pv: &mut PreviewState, key: u64, scratch: Session, cmds: Vec<(String, Value)>, colors: Colors) {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("preview".into()).spawn(move || {
        let _ = tx.send(compute(&scratch, &cmds, colors));
    });
    match spawned {
        Ok(_) => pv.running = Some((key, rx)),
        Err(e) => pv.apply(key, Err(e.to_string())),
    }
}

/// No threads on the web: compute right away.
#[cfg(target_arch = "wasm32")]
fn spawn(pv: &mut PreviewState, key: u64, scratch: Session, cmds: Vec<(String, Value)>, colors: Colors) {
    pv.apply(key, compute(&scratch, &cmds, colors));
}

/// Evaluate the commands on the scratch session and build the preview scene.
/// When the feature itself fails (a boolean the kernel can't do yet), the tool body alone is
/// shown with the error, so the shape can still be judged.
pub fn compute(s: &Session, cmds: &[(String, Value)], colors: Colors) -> Job {
    let (p, error) = match s.preview(cmds) {
        Ok(p) => (p, None),
        Err(e) => match as_new_body(cmds).and_then(|alt| s.preview(&alt).ok()) {
            Some(p) => (p, Some(e.to_string())),
            None => return Err(e.to_string()),
        },
    };
    let mut b = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&p.before, &p.after, colors)))
        .map_err(|_| "the preview failed".to_string())?;
    b.error = error;
    Ok(b)
}

/// The same commands making a new body instead of joining, cutting or intersecting.
fn as_new_body(cmds: &[(String, Value)]) -> Option<Vec<(String, Value)>> {
    let mut out = cmds.to_vec();
    let mut changed = false;
    for (_, p) in &mut out {
        let p = if p.get("command").is_some() { p.get_mut("params")? } else { p };
        if let Some(op) = p.get_mut("operation")
            && op.as_str().is_some_and(|o| o != "new")
        {
            *op = Value::from("new");
            changed = true;
        }
    }
    changed.then_some(out)
}

fn same(a: &Mesh, b: &Mesh) -> bool {
    a.triangles.len() == b.triangles.len() && a.positions == b.positions
}

/// The preview scene: changed and new bodies (new surface tinted) and, as a ghost, the old
/// version of changed or removed bodies.
pub fn build(before: &ModelState, after: &ModelState, c: Colors) -> Built {
    let mut sc = GpuScene::default();
    let mut replaced = Vec::new();
    for a in &after.bodies {
        let am = a.mesh();
        let old = before.body(&a.name).map(|b| b.mesh());
        if old.as_ref().is_some_and(|bm| same(bm, &am)) {
            continue;
        }
        let nf = am.tri_face.iter().max().map_or(0, |m| *m as usize + 1);
        let added: Vec<bool> = (0..nf).map(|f| old.as_ref().is_none_or(|bm| face_is_new(&am, f, bm))).collect();
        for (t, f) in am.triangles.iter().zip(&am.tri_face) {
            let col = if added.get(*f as usize).copied().unwrap_or(true) { c.added } else { c.body };
            for k in t {
                let i = *k as usize;
                if let (Some(p), Some(n)) = (am.positions.get(i), am.normals.get(i)) {
                    sc.tri(p.to_f32(), n.to_f32(), col);
                }
            }
        }
        for (ei, e) in am.edges.iter().enumerate() {
            if am.seams.get(ei).copied().unwrap_or(false) {
                continue;
            }
            for w in e.windows(2) {
                sc.line(w[0].to_f32(), w[1].to_f32(), c.edge, 1.3, false);
            }
        }
        if old.is_some() {
            replaced.push(a.name.clone());
        }
    }
    for b in &before.bodies {
        let gone = after.body(&b.name).is_none();
        if !gone && !replaced.contains(&b.name) {
            continue;
        }
        let m = b.mesh();
        for t in &m.triangles {
            for k in t {
                let i = *k as usize;
                if let (Some(p), Some(n)) = (m.positions.get(i), m.normals.get(i)) {
                    sc.ghost_tri(p.to_f32(), n.to_f32(), c.removed);
                }
            }
        }
        if gone {
            replaced.push(b.name.clone());
        }
    }
    Built { scene: sc, replaced, error: None }
}

/// Is face `f` of `m` new surface, i.e. not on the old mesh? Decided by a vote of points on it
/// (triangle centroids spread over the face).
fn face_is_new(m: &Mesh, f: usize, old: &Mesh) -> bool {
    let cents: Vec<Vec3> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf as usize == f)
        .filter_map(|(t, _)| m.tri(t))
        .map(|[a, b, c]| (a + b + c) / 3.0)
        .collect();
    if cents.is_empty() {
        return true;
    }
    let step = cents.len().div_ceil(9);
    let samples: Vec<&Vec3> = cents.iter().step_by(step.max(1)).collect();
    let off = samples.iter().filter(|p| !on_surface(old, ***p)).count();
    off * 3 > samples.len()
}

/// Does `p` lie on the surface of mesh `m` (within its tessellation tolerance)?
fn on_surface(m: &Mesh, p: Vec3) -> bool {
    let bb = m.bounds();
    let tol = (bb.diagonal() * 2e-3).max(1e-4);
    if bb.is_empty() {
        return false;
    }
    m.triangles.iter().filter_map(|t| m.tri(t)).any(|[a, b, c]| point_triangle_dist(p, a, b, c) < tol)
}

pub fn point_triangle_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn colors() -> Colors {
        Colors { body: [1, 1, 1, 255], added: [2, 2, 2, 255], removed: [3, 3, 3, 100], edge: [0, 0, 0, 255] }
    }

    fn count(bytes: &[u8], stride: usize, col: [u8; 4]) -> usize {
        bytes.chunks(stride).filter(|v| v.get(24..28) == Some(&col[..])).count()
    }

    fn plate() -> Session {
        let mut s = Session::default();
        for (id, p) in [
            ("SketchCreate", json!({"plane": "XY"})),
            ("ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]})),
            ("SketchStop", json!({})),
            ("Extrude", json!({"distance": 20})),
        ] {
            s.execute(id, &p).unwrap();
        }
        s
    }

    /// A fillet preview stands in for the body: the round face is new, the rest is kept, and the
    /// old body is the ghost. The session is unchanged.
    #[test]
    fn fillet_preview_scene() {
        let s = plate();
        let (rev, undo) = (s.revision, s.undo.len());
        let b = compute(&s, &[("FusionFilletEdgesCommand".into(), json!({"edges": [[0, 0, 10]], "radius": 3}))], colors()).unwrap();
        assert_eq!(b.replaced, vec!["Body1".to_string()]);
        let added = count(&b.scene.tris, crate::gpu::TRI_SIZE, colors().added);
        let kept = count(&b.scene.tris, crate::gpu::TRI_SIZE, colors().body);
        assert!(added > 0 && kept > 0, "added {added} kept {kept}");
        assert!(!b.scene.ghost.is_empty());
        assert_eq!(s.revision, rev);
        assert_eq!(s.undo.len(), undo);
    }

    /// A new body is all new surface and replaces nothing; a bad value is an error.
    #[test]
    fn extrude_preview_scene() {
        let mut s = Session::default();
        s.execute("SketchCreate", &json!({"plane": "XY"})).unwrap();
        s.execute("CircleCenterRadius", &json!({"center": [0, 0], "radius": 5})).unwrap();
        s.execute("SketchStop", &json!({})).unwrap();
        let b = compute(&s, &[("Extrude".into(), json!({"distance": "15 mm"}))], colors()).unwrap();
        assert!(b.replaced.is_empty());
        assert_eq!(count(&b.scene.tris, crate::gpu::TRI_SIZE, colors().body), 0);
        assert!(b.scene.ghost.is_empty());
        assert!(compute(&s, &[("Extrude".into(), json!({"distance": "1 +"}))], colors()).is_err());
        assert!(s.model.state().bodies.is_empty());
    }
}
