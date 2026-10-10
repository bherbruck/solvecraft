//! The SolveCraft engine.
//!
//! Every user-visible action is a command with a stable, dotted, lower-case id
//! (`solid.extrude`, `sketch.create`, `solid.fillet`, `timeline.rollback`). Commands take JSON
//! parameters and never open dialogs; the UI, the command palette, scripts, the CLI and the
//! control channel all run the same commands through [`Session::execute`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cmd;
pub mod frames;
pub mod fuzz;
pub mod legacy_ids;
pub mod licences;
pub mod params;
pub mod recovery;
pub mod sample;
pub mod view;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solvecraft_doc::{Document, Model, ModelState};
use solvecraft_geom::Vec3;

pub use cmd::{CommandInfo, CommandSpec, auto_operation, command_specs, find_command};
pub use solvecraft_doc as doc;
pub use solvecraft_geom as geom;
pub use solvecraft_io as io;
pub use solvecraft_kernel as kernel;
pub use solvecraft_render as render;
pub use solvecraft_sketch as sketch;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("`{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("{0}")]
    Other(String),
    /// A command panicked; the document was kept as it was (a bug: please report it).
    #[error("internal error in `{0}` (the design was kept as it was): {1}")]
    Internal(String, String),
}

impl From<solvecraft_doc::DocError> for EngineError {
    fn from(e: solvecraft_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solvecraft_sketch::SketchError> for EngineError {
    fn from(e: solvecraft_sketch::SketchError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solvecraft_kernel::KernelError> for EngineError {
    fn from(e: solvecraft_kernel::KernelError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solvecraft_io::IoError> for EngineError {
    fn from(e: solvecraft_io::IoError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Something selected in the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Sel {
    Body {
        name: String,
    },
    /// An edge, identified by a point on it.
    Edge {
        body: String,
        index: usize,
        point: Vec3,
    },
    Face {
        body: String,
        index: usize,
        point: Vec3,
    },
    Feature {
        id: u64,
    },
    /// A sketch curve or point (by id) of the active sketch.
    SketchCurve {
        id: String,
    },
    SketchPoint {
        id: String,
    },
    /// A constraint or dimension (by id) of the active sketch.
    SketchConstraint {
        id: String,
    },
    /// A closed profile of a sketch (by index).
    Profile {
        sketch: u64,
        index: usize,
    },
    /// An origin plane (XY, XZ, YZ) or a construction plane (by feature name).
    Plane {
        name: String,
    },
    /// An origin axis (X, Y, Z).
    Axis {
        name: String,
    },
    /// A body vertex at a point.
    Vertex {
        body: String,
        point: Vec3,
    },
}

/// One undo step.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub label: String,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
}

/// What some commands would make, evaluated on a scratch copy of a session
/// ([`Session::preview`]). The session itself is not changed.
#[derive(Clone, Debug)]
pub struct Preview {
    /// The model now.
    pub before: Arc<ModelState>,
    /// The model after the commands.
    pub after: Arc<ModelState>,
}

/// An editing session on one design.
pub struct Session {
    pub doc: Arc<Document>,
    pub model: Model,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    /// Sketch feature being edited (sketch mode).
    pub active_sketch: Option<u64>,
    pub selection: Vec<Sel>,
    pub path: Option<String>,
    saved: Arc<Document>,
    /// Messages for the command line / log.
    pub log: Vec<String>,
    /// Increments on every document change (views use it to refresh).
    pub revision: u64,
    /// Project a face's edges into a sketch created on it (Fusion's default; `sketch.auto_project`).
    pub auto_project: bool,
    /// The component new sketches and features go into (0 = the root).
    pub active_component: u64,
    /// Occurrence moves not captured yet (Capture Position keeps them, a recompute drops them).
    pub pending_moves: std::collections::BTreeMap<u64, solvecraft_doc::Mat>,
    world_cache: std::sync::Mutex<Option<(u64, Arc<solvecraft_doc::ModelState>)>>,
    /// Features copied with `timeline.copy` (pasted by `timeline.paste`).
    pub clipboard: Vec<solvecraft_doc::Feature>,
    /// Section Analysis: a view cut by a plane (origin, unit normal; the normal side is hidden).
    pub section: Option<(Vec3, Vec3)>,
    /// Surface analysis shading the model (zebra, draft, curvature map).
    pub analysis: Option<SurfaceAnalysis>,
    /// What is hidden or shown in the view (`browser.visibility`).
    pub visibility: Visibility,
    /// States to go back to (`edit.checkpoint`, `edit.restore_checkpoint`): how batches of
    /// commands are undone however long the undo history is.
    pub(crate) checkpoints: Vec<Checkpoint>,
    next_checkpoint: u64,
}

/// The design, active sketch and undo history at an `edit.checkpoint`.
#[derive(Clone, Debug)]
pub(crate) struct Checkpoint {
    pub id: u64,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
}

/// Items hidden or shown one by one (view state, not part of the design and not an undo step).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Visibility {
    /// Bodies by name.
    pub hidden_bodies: Vec<String>,
    /// Origin items (O, X, Y, Z, XY, XZ, YZ) and construction geometry, by name.
    pub hidden_origin: Vec<String>,
    /// Sketches by feature id.
    pub hidden_sketches: Vec<u64>,
    /// Finished sketches shown although a feature uses them.
    pub shown_sketches: Vec<u64>,
}

/// A way of shading the model's faces to judge their shape (view state, not part of the design).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SurfaceAnalysis {
    /// Reflected stripes: `stripes` across the view.
    Zebra { stripes: f64 },
    /// Faces by draft angle to `pull`: past +`angle` green, past −`angle` red, between yellow.
    Draft { pull: Vec3, angle: f64 },
    /// Colour by curvature: blue flat, through green, to red at 1 / `radius`.
    Curvature { radius: f64 },
    /// Mirror finish reflecting a studio (sky, horizon, floor, light panels).
    Environment,
    /// Green where a tool coming along `dir` reaches the face (facing it, nothing above), red
    /// elsewhere.
    Access { dir: Vec3 },
}

const MAX_UNDO: usize = 200;
/// Checkpoints kept (the oldest go first).
const MAX_CHECKPOINTS: usize = 16;
const MAX_LOG: usize = 2000;

impl Default for Session {
    fn default() -> Self {
        Session::new(Document::new("Untitled"))
    }
}

impl Session {
    pub fn new(doc: Document) -> Self {
        let doc = Arc::new(doc);
        let mut model = Model::new();
        model.evaluate(&doc);
        Session {
            saved: doc.clone(),
            doc,
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            active_sketch: None,
            selection: Vec::new(),
            path: None,
            log: Vec::new(),
            revision: 1,
            auto_project: true,
            active_component: 0,
            pending_moves: Default::default(),
            world_cache: Default::default(),
            clipboard: Vec::new(),
            section: None,
            analysis: None,
            checkpoints: Vec::new(),
            next_checkpoint: 1,
            visibility: Visibility::default(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.doc, &self.saved) && *self.doc != *self.saved
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.doc.clone();
    }
    /// The design has changes not in its file (a recovered design).
    pub fn mark_unsaved(&mut self) {
        self.saved = Arc::new(Document::new(""));
    }

    pub fn echo(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
        if self.log.len() > MAX_LOG {
            let n = self.log.len() - MAX_LOG;
            self.log.drain(..n);
        }
    }

    /// Mutable document (copy-on-write).
    pub fn doc_mut(&mut self) -> &mut Document {
        Arc::make_mut(&mut self.doc)
    }

    /// Re-evaluate the model after a document change.
    /// The model placed in the world through component occurrences (and uncaptured moves): what
    /// to display, measure, pick and export. The same as `model.state()` without components.
    pub fn world_state(&self) -> Arc<solvecraft_doc::ModelState> {
        let st = self.model.state();
        if self.doc.occurrences.is_empty() {
            return st;
        }
        if let Ok(c) = self.world_cache.lock()
            && let Some((rev, w)) = c.as_ref()
            && *rev == self.revision
        {
            return w.clone();
        }
        let w = if self.pending_moves.is_empty() {
            solvecraft_doc::world_state(&self.doc, &st)
        } else {
            let mut d = (*self.doc).clone();
            for o in &mut d.occurrences {
                if let Some(m) = self.pending_moves.get(&o.id) {
                    o.transform = *m;
                }
            }
            solvecraft_doc::world_state(&d, &st)
        };
        let w = Arc::new(w);
        if let Ok(mut c) = self.world_cache.lock() {
            *c = Some((self.revision, w.clone()));
        }
        w
    }

    pub fn refresh(&mut self) {
        self.model.evaluate(&self.doc);
        self.revision += 1;
        // Joints follow the geometry their origins snap to.
        if !self.doc.assembly.joints.is_empty() {
            cmd::joints::resolve(self);
        }
        if let Some(id) = self.active_sketch
            && self.doc.feature(id).is_none()
        {
            self.active_sketch = None;
        }
    }

    /// Run a command with JSON parameters (no dialogs). Undoable commands record an undo step
    /// when they change the document; a failing or panicking command leaves it unchanged.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        (spec.enabled)(self).map_err(|why| EngineError::Disabled(spec.id.to_string(), why))?;
        let before = Snapshot { label: spec.label.to_string(), doc: self.doc.clone(), active_sketch: self.active_sketch };
        let null = Value::Object(Default::default());
        let params = if params.is_null() { &null } else { params };
        // Picks in the world go into their component's frame (once: commands run by commands
        // already have them there).
        let (_nested, outermost) = frames::Nested::enter(spec.id);
        let mapped = if outermost { frames::to_local(self, spec.id, params) } else { None };
        let params = mapped.as_ref().unwrap_or(params);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, params)));
        let r = match r {
            Ok(r) => r,
            Err(p) => {
                let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
                Err(EngineError::Internal(spec.id.to_string(), msg))
            }
        };
        let changed = !Arc::ptr_eq(&self.doc, &before.doc);
        match r {
            Ok(v) => {
                if changed {
                    if spec.undoable {
                        self.undo.push(before);
                        if self.undo.len() > MAX_UNDO {
                            self.undo.remove(0);
                        }
                        self.redo.clear();
                    }
                    self.refresh();
                }
                Ok(v)
            }
            Err(e) => {
                if changed {
                    self.doc = before.doc;
                    self.active_sketch = before.active_sketch;
                    self.refresh();
                }
                Err(e)
            }
        }
    }

    /// Remember the design, active sketch and undo history; returns the checkpoint's id.
    pub fn checkpoint(&mut self) -> u64 {
        let id = self.next_checkpoint;
        self.next_checkpoint += 1;
        self.checkpoints.push(Checkpoint {
            id,
            doc: self.doc.clone(),
            active_sketch: self.active_sketch,
            undo: self.undo.clone(),
            redo: self.redo.clone(),
        });
        if self.checkpoints.len() > MAX_CHECKPOINTS {
            self.checkpoints.remove(0);
        }
        id
    }

    /// Go back to checkpoint `id` as if nothing since had happened (the undo history too). It
    /// and later checkpoints are used up.
    pub fn restore_checkpoint(&mut self, id: u64) -> Result<()> {
        let i = self.checkpoints.iter().position(|c| c.id == id).ok_or_else(|| EngineError::Other(format!("no checkpoint {id}")))?;
        let Some(c) = self.checkpoints.drain(i..).next() else { return Err(EngineError::Other(format!("no checkpoint {id}"))) };
        self.doc = c.doc;
        self.active_sketch = c.active_sketch;
        self.undo = c.undo;
        self.redo = c.redo;
        self.selection.clear();
        self.refresh();
        Ok(())
    }

    /// Forget checkpoint `id` (and later ones) without going back.
    pub fn drop_checkpoint(&mut self, id: u64) {
        if let Some(i) = self.checkpoints.iter().position(|c| c.id == id) {
            self.checkpoints.truncate(i);
        }
    }

    /// A scratch copy of the session (same design and model, no history or log) to try
    /// commands on without touching this one.
    pub fn scratch(&self) -> Session {
        Session {
            doc: self.doc.clone(),
            model: self.model.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
            active_sketch: self.active_sketch,
            selection: Vec::new(),
            path: None,
            saved: self.saved.clone(),
            log: Vec::new(),
            revision: self.revision,
            auto_project: self.auto_project,
            active_component: self.active_component,
            pending_moves: self.pending_moves.clone(),
            world_cache: Default::default(),
            clipboard: self.clipboard.clone(),
            section: None,
            analysis: None,
            checkpoints: Vec::new(),
            next_checkpoint: 1,
            visibility: Visibility::default(),
        }
    }

    /// Live preview: run `commands` (id, parameters) on a scratch copy and return the model
    /// before and after. Nothing in this session changes (document, undo history, selection).
    /// A `timeline.redefine` is shown even while the timeline is rolled back to the feature
    /// being edited.
    pub fn preview(&self, commands: &[(String, Value)]) -> Result<Preview> {
        self.scratch().preview_in_place(commands).map(|after| Preview { before: self.model.state(), after })
    }

    fn preview_in_place(mut self, commands: &[(String, Value)]) -> Result<Arc<ModelState>> {
        if commands.is_empty() {
            return Err(EngineError::Other("nothing to preview".into()));
        }
        for (id, params) in commands {
            let r = self.execute(id, params)?;
            if id == "timeline.redefine"
                && let Some(fid) = r.get("feature").and_then(Value::as_u64)
                && let Some(idx) = self.doc.feature_index(fid)
                && self.doc.marker.is_some_and(|m| m <= idx)
            {
                self.doc_mut().marker = Some(idx + 1);
                self.refresh();
                if let Some(e) = self.model.result(fid).and_then(|r| r.error.clone()) {
                    return Err(EngineError::Other(e));
                }
            }
        }
        Ok(self.model.state())
    }

    /// Measure one or two selections (see `inspect.measure` with `items`); errors come back as
    /// `{"error": …}`.
    pub fn measure_items(&self, items: &[Sel]) -> Value {
        let v = serde_json::to_value(items).unwrap_or(Value::Null);
        cmd::measure_items(self, &v).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    }

    /// Run a script: a JSON array of `{"command": id, "params": {...}}` (or an object with a
    /// `commands` array). Stops at the first error. Returns each command's result.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        let list = match script {
            Value::Array(a) => a.clone(),
            Value::Object(o) => {
                o.get("commands").and_then(Value::as_array).cloned().ok_or_else(|| EngineError::Other("script: missing `commands` array".into()))?
            }
            _ => return Err(EngineError::Other("script must be a JSON array or {\"commands\": [...]}".into())),
        };
        if list.len() > 100_000 {
            return Err(EngineError::Other("script too long".into()));
        }
        let mut out = Vec::new();
        for (i, c) in list.iter().enumerate() {
            let id = c
                .get("command")
                .or_else(|| c.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EngineError::Other(format!("script step {}: missing `command`", i + 1)))?;
            let p = c.get("params").cloned().unwrap_or(Value::Null);
            let r = self.execute(id, &p).map_err(|e| EngineError::Other(format!("step {} ({id}): {e}", i + 1)))?;
            out.push(r);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod construct_tests;
#[cfg(test)]
mod delete_face_tests;
#[cfg(test)]
mod extent_tests;
#[cfg(test)]
mod pattern_suppress_tests;
#[cfg(test)]
mod profile_refs_tests;
#[cfg(test)]
mod split_face_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thread_tests;
