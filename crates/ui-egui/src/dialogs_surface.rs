//! Dialogs for the surface tools and face splitting (#19): Patch, Stitch, Thicken, Extend,
//! Trim and Split Face. Each has a live preview and Edit Feature.

use serde_json::{Map, Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::doc::{FaceAt, FeatureKind, PlaneRef};
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Kind, OP_LABELS, OPS, combo, field, op_index, row_label};
use crate::selection::{BODIES, EDGES, FACES, PLANES, PROFILES, SelInput};

/// A surface tool dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub enum Surf {
    Patch,
    Stitch { tolerance: String },
    Thicken { thickness: String, symmetric: bool, operation: usize },
    Extend { distance: String },
    Trim,
    SplitFace,
}

impl Surf {
    pub fn title(&self) -> &'static str {
        match self {
            Surf::Patch => "PATCH",
            Surf::Stitch { .. } => "STITCH",
            Surf::Thicken { .. } => "THICKEN",
            Surf::Extend { .. } => "EXTEND",
            Surf::Trim => "TRIM",
            Surf::SplitFace => "SPLIT FACE",
        }
    }

    /// The value on the canvas.
    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(match self {
            Surf::Thicken { thickness, .. } => ("Thickness", ValueKind::Length, thickness),
            Surf::Extend { distance } => ("Distance", ValueKind::Length, distance),
            _ => return None,
        })
    }
}

fn inputs(k: &Surf) -> Vec<SelInput> {
    match k {
        Surf::Patch => vec![SelInput::new("Boundary", PROFILES | EDGES, true)],
        Surf::Stitch { .. } => vec![SelInput::new("Surfaces", BODIES, true)],
        Surf::Thicken { .. } => vec![SelInput::new("Surfaces", BODIES, true)],
        Surf::Extend { .. } => vec![SelInput::new("Edges", EDGES, true)],
        Surf::Trim => vec![
            SelInput::new("Surface", BODIES, false),
            SelInput::new("Trimming tool", PLANES | FACES, false),
            SelInput::new("Side to keep", FACES, false),
        ],
        Surf::SplitFace => vec![SelInput::new("Faces to split", FACES, true), SelInput::new("Splitting tool", PLANES | FACES, false)],
    }
}

/// The dialog for a surface command.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let k = match id {
        "surface.patch" => Surf::Patch,
        "surface.stitch" => Surf::Stitch { tolerance: String::new() },
        "surface.thicken" => Surf::Thicken { thickness: "2 mm".into(), symmetric: false, operation: 0 },
        "surface.extend" => Surf::Extend { distance: "10 mm".into() },
        "surface.trim" => Surf::Trim,
        "solid.split_face" => Surf::SplitFace,
        _ => return None,
    };
    let i = inputs(&k);
    Some((Kind::Surface(k), i))
}

/// The rows after the inputs; true when Enter was pressed in a field.
pub fn rows(ui: &mut egui::Ui, k: &mut Surf) -> bool {
    let mut enter = false;
    match k {
        Surf::Stitch { tolerance } => {
            row_label(ui, "Tolerance");
            let r = ui.add(egui::TextEdit::singleline(tolerance).hint_text("tight"));
            crate::params_dialog::complete(ui, &r, tolerance);
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.end_row();
        }
        Surf::Thicken { thickness, symmetric, operation } => {
            row_label(ui, "Thickness");
            enter |= field(ui, thickness);
            ui.end_row();
            row_label(ui, "Direction");
            let mut dir = usize::from(*symmetric);
            combo(ui, "th_dir", &["One Side", "Symmetric"], &mut dir);
            *symmetric = dir == 1;
            ui.end_row();
            row_label(ui, "Operation");
            combo(ui, "th_op", &OP_LABELS, operation);
            ui.end_row();
        }
        Surf::Extend { distance } => {
            row_label(ui, "Distance");
            enter |= field(ui, distance);
            ui.end_row();
        }
        Surf::Patch | Surf::Trim | Surf::SplitFace => {}
    }
    enter
}

fn items(inputs: &[SelInput], i: usize) -> &[Sel] {
    inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn bodies(inputs: &[SelInput], i: usize) -> Vec<String> {
    items(inputs, i).iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect()
}

/// A splitting or trimming tool: a plane by name, a planar face as its plane, or any other
/// face as {body, point} (its surface extended).
fn tool(app: &SolveApp, sel: Option<&Sel>) -> Result<(&'static str, Value), String> {
    match sel {
        Some(Sel::Plane { name }) => Ok(("plane", json!(name))),
        Some(Sel::Face { body, index, point }) => match crate::dialogs::planar_face(&app.session, body, *index) {
            Some((_, n)) => Ok(("plane", json!({"origin": pt(*point), "normal": pt(n)}))),
            None => Ok(("tool", json!({"body": body, "point": pt(*point)}))),
        },
        _ => Err("pick a plane or a face as the tool".into()),
    }
}

/// The command a surface dialog makes.
pub fn commands(app: &SolveApp, k: &Surf, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let need = |i: usize, what: &str| if items(inputs, i).is_empty() { Err(format!("select {what} first")) } else { Ok(()) };
    let (cmd, mut p) = match k {
        Surf::Patch => {
            need(0, "a boundary (sketch profiles or a loop of edges)")?;
            let profiles: Vec<(u64, usize)> =
                items(inputs, 0).iter().filter_map(|x| if let Sel::Profile { sketch, index } = x { Some((*sketch, *index)) } else { None }).collect();
            if let Some((sk, _)) = profiles.first() {
                let idx: Vec<usize> = profiles.iter().filter(|(s, _)| s == sk).map(|(_, i)| *i).collect();
                ("surface.patch", json!({"sketch": sk, "profiles": idx}))
            } else {
                let edges: Vec<Value> =
                    items(inputs, 0).iter().filter_map(|x| if let Sel::Edge { point, .. } = x { Some(pt(*point)) } else { None }).collect();
                let body = items(inputs, 0).iter().find_map(|x| if let Sel::Edge { body, .. } = x { Some(body.clone()) } else { None });
                ("surface.patch", json!({"edges": edges, "body": body}))
            }
        }
        Surf::Stitch { tolerance } => {
            need(0, "the surfaces to stitch")?;
            let mut p = json!({ "bodies": bodies(inputs, 0) });
            if !tolerance.trim().is_empty() {
                p["tolerance"] = json!(tolerance);
            }
            ("surface.stitch", p)
        }
        Surf::Thicken { thickness, symmetric, operation } => {
            need(0, "the surfaces to thicken")?;
            (
                "surface.thicken",
                json!({"bodies": bodies(inputs, 0), "thickness": thickness, "symmetric": symmetric, "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
        }
        Surf::Extend { distance } => {
            need(0, "the edges to extend")?;
            let body =
                items(inputs, 0).iter().find_map(|x| if let Sel::Edge { body, .. } = x { Some(body.clone()) } else { None }).unwrap_or_default();
            let edges: Vec<Value> =
                items(inputs, 0).iter().filter_map(|x| if let Sel::Edge { point, .. } = x { Some(pt(*point)) } else { None }).collect();
            ("surface.extend", json!({"body": body, "edges": edges, "distance": distance}))
        }
        Surf::Trim => {
            need(0, "the surface to trim")?;
            need(1, "the trimming tool")?;
            need(2, "the side to keep (a point on it)")?;
            let body = bodies(inputs, 0).into_iter().next().unwrap_or_default();
            let (key, t) = tool(app, items(inputs, 1).first())?;
            let keep = items(inputs, 2)
                .first()
                .and_then(|x| if let Sel::Face { point, .. } = x { Some(pt(*point)) } else { None })
                .ok_or("pick the side to keep")?;
            let mut p = json!({"body": body, "keep": keep});
            p[key] = t;
            ("surface.trim", p)
        }
        Surf::SplitFace => {
            need(0, "the faces to split")?;
            need(1, "the splitting tool")?;
            let faces: Vec<Value> =
                items(inputs, 0).iter().filter_map(|x| if let Sel::Face { point, .. } = x { Some(pt(*point)) } else { None }).collect();
            let body = items(inputs, 0).iter().find_map(|x| if let Sel::Face { body, .. } = x { Some(body.clone()) } else { None });
            let (key, t) = tool(app, items(inputs, 1).first())?;
            let mut p = json!({"faces": faces, "body": body});
            p[key] = t;
            ("solid.split_face", p)
        }
    };
    if let Value::Object(m) = &mut p {
        for (k, v) in extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), p)])
}

/// A tool as a selection again: a plane, else the face at the stored point.
fn tool_sel(app: &SolveApp, plane: &PlaneRef, tool: Option<&FaceAt>) -> Option<Sel> {
    match tool {
        Some(t) => crate::dialogs::face_sel(&app.session, t.point),
        None => crate::dialogs::plane_sel(&app.session, plane),
    }
}

/// The dialog for editing a surface feature.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let s = &app.session;
    let edge = |p: &Vec3| crate::dialogs::edge_sel(s, *p);
    let (k, fill): (Surf, Vec<Vec<Sel>>) = match kind {
        FeatureKind::Patch { sketch, edges, .. } => {
            let mut sels: Vec<Sel> = edges.iter().filter_map(edge).collect();
            if let Some(sk) = sketch {
                let st = s.world_state();
                if let Some(ss) = st.sketch(*sk) {
                    sels.extend((0..ss.profiles.len()).map(|index| Sel::Profile { sketch: *sk, index }));
                }
            }
            (Surf::Patch, vec![sels])
        }
        FeatureKind::Stitch { bodies, tolerance } => (
            Surf::Stitch { tolerance: if tolerance.trim() == "0" { String::new() } else { tolerance.clone() } },
            vec![bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect()],
        ),
        FeatureKind::Thicken { bodies, thickness, symmetric, operation, .. } => (
            Surf::Thicken { thickness: thickness.clone(), symmetric: *symmetric, operation: op_index(operation) },
            vec![bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect()],
        ),
        FeatureKind::SurfaceExtend { edges, distance, .. } => {
            (Surf::Extend { distance: distance.clone() }, vec![edges.iter().filter_map(edge).collect()])
        }
        FeatureKind::SurfaceTrim { body, plane, tool, keep } => (
            Surf::Trim,
            vec![
                vec![Sel::Body { name: body.clone() }],
                tool_sel(app, plane, tool.as_ref()).into_iter().collect(),
                crate::dialogs::face_sel(s, *keep).into_iter().collect(),
            ],
        ),
        FeatureKind::SplitFace { faces, plane, tool, .. } => (
            Surf::SplitFace,
            vec![faces.iter().filter_map(|p| crate::dialogs::face_sel(s, *p)).collect(), tool_sel(app, plane, tool.as_ref()).into_iter().collect()],
        ),
        _ => return None,
    };
    let mut ins = inputs(&k);
    for (inp, items) in ins.iter_mut().zip(fill) {
        inp.items = items;
    }
    Some((Kind::Surface(k), ins, Map::new()))
}
